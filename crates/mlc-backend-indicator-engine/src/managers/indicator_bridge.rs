//! Indicator Bridge
//!
//! Converts indicators from the zengeld-chart-indicators catalog to
//! indicator definitions for rendering.

use crate::catalog::{
    UnifiedIndicatorCatalog, UnifiedIndicatorInfo, UnifiedCatalogStats,
    OutputSpec, OutputType as CatalogOutputType,
    HistogramStyle as CatalogHistogramStyle,
    IndicatorCategory as CatalogCategory,
    IndicatorRoleKind, IndicatorSignature,
    ParamType as CatalogParamType,
};

use super::indicator_manager::{
    IndicatorDefinition, IndicatorCategory, IndicatorOutput, IndicatorOutputType,
    IndicatorParam, HistogramStyle,
};

/// Streams a live `ComputeInstance` actually receives updates for, today.
/// `Bar` is always fed (`IndicatorCalculator::calculate_with_id` via
/// `IndicatorManager::calculate_for_bubble`). `Tick` joined in Arc D step 3
/// (`BackendCore::tick_writers`'s `TradeUpdate` arm calls
/// `IndicatorsMut::feed_stream(..., StreamEvent::Tick(...))` —
/// `mlc-backend/src/lib.rs`).
///
/// This is the SINGLE set [`all_streams_fed`] keys off — later Arc D steps
/// (`docs/mlc/plans/indicator-streams-arc-d-2026-09-25.md` §6 steps 5-6)
/// extend ONLY this array (`OrderBook` + `Liquidation` in step 5, `Funding`
/// + `OpenInterest` in step 6) rather than re-deriving the un-hide rule.
/// `OrderbookDelta` is deliberately absent even though `StreamEvent`/
/// `feed_stream_event` already dispatch it (step 1): no app-layer producer
/// calls `feed_stream(..., StreamEvent::OrderbookDelta(...))` anywhere yet
/// (§3.3's own risk flag — the order-book tracker has no "deltas since
/// cursor N" read), so its 4 primary entries stay hidden past step 6 too,
/// not just until a later step in this arc.
const FED_STREAMS: &[mli::data_loader::stream_kind::StreamKind] = &[
    mli::data_loader::stream_kind::StreamKind::Bar,
    mli::data_loader::stream_kind::StreamKind::Tick,
];

/// Whether EVERY stream `signature` declares (primary + every aux) is in
/// [`FED_STREAMS`]. A composite with one fed and one unfed stream would
/// otherwise silently never update its unfed half — the exact R6 symptom
/// for a different reason (Arc D plan §3.6 case 1).
fn all_streams_fed(signature: &IndicatorSignature) -> bool {
    FED_STREAMS.contains(&signature.input_stream)
        && signature.aux_streams.iter().all(|s| FED_STREAMS.contains(s))
}

/// Bridge between zengeld-chart-indicators catalog and indicator definitions
pub struct IndicatorBridge {
    catalog: UnifiedIndicatorCatalog,
}

impl IndicatorBridge {
    /// Create a new indicator bridge
    pub fn new() -> Self {
        Self {
            catalog: UnifiedIndicatorCatalog::new(),
        }
    }

    /// Convert unified indicator info to indicator definition
    pub fn to_definition(&self, info: &UnifiedIndicatorInfo) -> IndicatorDefinition {
        let rendering = info.rendering.as_ref();

        // Map category
        let category = Self::map_category(info.category());

        // Map outputs
        let outputs = if let Some(r) = rendering {
            r.outputs.iter().map(Self::map_output).collect()
        } else {
            vec![IndicatorOutput::line("value", "Value", "#2196F3")]
        };

        // Extract parameters from signature (simplified - main period param)
        let params = self.extract_params(info);

        // Get rendering properties with defaults
        //
        // The host-rendered entries never have rendering metadata (their
        // `machine_id` is `None`, so `UnifiedIndicatorCatalog::get` resolves
        // no `RenderingMetadata` — see `unified.rs`), so the fallback below
        // would put every one of them in a pane. They must say for
        // themselves where they paint.
        //
        // `overlay` is the VISUALISATION SPACE: true = paint on the price
        // plot (pane 0), false = a pane of its own (pane 1). It is NOT the
        // category — the host-rendered entries all share one category and
        // differ only in where they paint, which each signature states as
        // `metadata("kind")`. The `overlay_` id prefix exists to avoid
        // colliding with compute-indicator ids and says nothing about space.
        let overlay = match info.signature.metadata.get("kind").map(String::as_str) {
            Some("opaque_subpane") => false,
            Some("opaque_overlay") => true,
            _ => rendering.map(|r| r.overlay).unwrap_or(false),
        };
        let bounds = rendering.and_then(|r| r.bounds);
        let zero_baseline = rendering.map(|r| r.zero_baseline).unwrap_or(false);
        let histogram_style = rendering
            .map(|r| Self::map_histogram_style(r.histogram_style))
            .unwrap_or(HistogramStyle::FromBottom);
        let precision = rendering.map(|r| r.precision).unwrap_or(4);

        // Get description
        let description = info.signature.description.clone();

        // `short_name` is what the chart legend row and the settings-modal
        // title render. For a normal indicator the catalog id IS the label
        // ("RSI", "SMA", "VOLUME"), but the opaque overlays are keyed by a
        // machine-shaped id (`overlay_liquidation_heatmap`) that reads as a
        // leak of internals on screen — use their human name instead.
        let short_name = if info.id().starts_with("overlay_") {
            info.signature.name.clone()
        } else {
            info.id().to_string()
        };

        IndicatorDefinition {
            type_id: info.id().to_lowercase(),
            name: info.signature.name.clone(),
            short_name,
            description,
            category,
            params,
            outputs,
            overlay,
            precision,
            bounds,
            zero_baseline,
            histogram_style,
            // Use machine_id directly from signature (typed enum - no string parsing!)
            machine_id: info.signature.machine_id,
            // Declared data streams, verbatim from the catalog signature —
            // primary first, then auxiliaries. The host arms subscriptions
            // from this list; see `ChartApp::arm_indicator_feeds`.
            streams: std::iter::once(info.signature.input_stream)
                .chain(info.signature.aux_streams.iter().copied())
                .collect(),
            // A compute indicator (`machine_id: Some(..)`) is only visible
            // once every stream it declares (primary + aux) is one
            // `IndicatorCalculator`/`IndicatorManager::feed_stream_for_bubble`
            // actually feeds — see [`FED_STREAMS`]/[`all_streams_fed`].
            // Opaque host-driven overlays (`machine_id: None` — dom heatmap,
            // funding rate, liquidation heatmap/projection, OI delta) never
            // go through `IndicatorCalculator`/`ComputeInstance` at all;
            // they're rendered straight off the live data bridge regardless
            // of their declared `input_stream`, so they're always fed.
            // Catalog-listing consumers filter on this; `self.definitions`
            // (keyed lookup for instance creation / preset restore) is
            // unaffected.
            fed_by_engine: info.signature.machine_id.is_none()
                || all_streams_fed(&info.signature),
            // Pattern-recognition entries (candlestick, fractals, ...)
            // belong to the left-toolbar auto-detector scans, not the
            // indicator catalog — filtered by the same consumers.
            is_pattern_role: matches!(info.signature.role_kind, Some(IndicatorRoleKind::Pattern)),
        }
    }

    /// Extract parameters from indicator signature
    fn extract_params(&self, info: &UnifiedIndicatorInfo) -> Vec<IndicatorParam> {
        let mut params = Vec::new();

        // Extract constraints from signature's constraint set
        for constraint in &info.signature.constraints.constraints {
            match &constraint.param_type {
                CatalogParamType::USize => {
                    let min: i32 = constraint.min.as_ref()
                        .and_then(|v| v.as_usize())
                        .unwrap_or(1) as i32;
                    let max: i32 = constraint.max.as_ref()
                        .and_then(|v| v.as_usize())
                        .unwrap_or(500) as i32;
                    let default: i32 = constraint.default.as_ref()
                        .and_then(|v| v.as_usize())
                        .unwrap_or(14) as i32;

                    params.push(IndicatorParam::int(&constraint.name, &constraint.name, default, min, max));
                }
                CatalogParamType::F64 => {
                    let min: f64 = constraint.min.as_ref()
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    let max: f64 = constraint.max.as_ref()
                        .and_then(|v| v.as_f64())
                        .unwrap_or(100.0);
                    let default: f64 = constraint.default.as_ref()
                        .and_then(|v| v.as_f64())
                        .unwrap_or(1.0);

                    params.push(IndicatorParam::float(&constraint.name, &constraint.name, default, min, max));
                }
                CatalogParamType::Bool => {
                    let default: bool = constraint.default.as_ref()
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    params.push(IndicatorParam::bool(&constraint.name, &constraint.name, default));
                }
                CatalogParamType::MaType => {
                    // MA type selector (dropdown with MA types)
                    let options = vec![
                        "SMA".to_string(),
                        "EMA".to_string(),
                        "WMA".to_string(),
                        "RMA".to_string(),
                        "DEMA".to_string(),
                        "TEMA".to_string(),
                        "HMA".to_string(),
                        "TMA".to_string(),
                        "VWMA".to_string(),
                        "AMA".to_string(),
                    ];

                    // Get default MA type from constraint
                    let default_index = constraint.default.as_ref()
                        .and_then(|v| v.as_ma_type())
                        .map(|ma_type| {
                            // Map MovingAverageType to index
                            use crate::bar_indicators::average::moving_average::MovingAverageType;
                            match ma_type {
                                MovingAverageType::SMA => 0,
                                MovingAverageType::EMA => 1,
                                MovingAverageType::WMA => 2,
                                MovingAverageType::RMA => 3,
                                MovingAverageType::DEMA => 4,
                                MovingAverageType::TEMA => 5,
                                MovingAverageType::HMA => 6,
                                MovingAverageType::TMA => 7,
                                MovingAverageType::VWMA => 8,
                                MovingAverageType::AMA => 9,
                                MovingAverageType::VWAP => 1, // Map to EMA if VWAP selected
                            }
                        })
                        .unwrap_or(1); // Default to EMA

                    let display_name = Self::humanize_param_name(&constraint.name);
                    params.push(IndicatorParam::select(&constraint.name, &display_name, options, default_index));
                }
                CatalogParamType::Source => {
                    // Component-level source parameter (e.g., fast_source, slow_source)
                    let display_name = Self::humanize_param_name(&constraint.name);
                    params.push(IndicatorParam::source(&constraint.name, &display_name));
                }
                CatalogParamType::StringList => {
                    // The venue set of an aggregating overlay. The catalog
                    // declares only that it exists — the offered venues come
                    // from the host, filtered by this signature's own
                    // `input_stream`, at the moment the picker opens.
                    let display_name = Self::humanize_param_name(&constraint.name);
                    params.push(IndicatorParam::venues(&constraint.name, &display_name));
                }
                // A string that names its allowed values IS a dropdown. Only
                // `MaType` used to reach the modal as one, so any other
                // enumerated setting had nowhere to live — which is why the
                // Volume Profile's mode spent its life as an unreachable
                // toolbar action instead of a parameter.
                CatalogParamType::String if !constraint.options.is_empty() => {
                    let display_name = Self::humanize_param_name(&constraint.name);
                    let default = constraint.default.as_ref()
                        .and_then(|v| v.as_string().map(|s| s.to_string()))
                        .unwrap_or_default();
                    let default_index = constraint.options.iter()
                        .position(|o| *o == default)
                        .unwrap_or(0);
                    params.push(IndicatorParam::select(
                        &constraint.name,
                        &display_name,
                        constraint.options.clone(),
                        default_index,
                    ));
                }
                _ => {
                    // Skip other types (a free-text String, etc.)
                }
            }
        }

        // Add global source parameter ONLY for PriceOnly indicators (backward compatibility)
        // This is the main "source" parameter that applies to the whole indicator
        //
        // The opaque host-driven overlays are excluded: they compute nothing
        // from a price series (`machine_id: None` — the host paints them from
        // its own live feed), so a "Source: close" row in their settings modal
        // is a control that does nothing. Their `source_type` still reads
        // `PriceOnly` by default in the catalog, hence the explicit skip
        // rather than a signature change.
        use crate::catalog::SourceType;
        if info.signature.source_type == SourceType::PriceOnly && !info.id().starts_with("overlay_") {
            // Only add if there's no component-level source already
            if !params.iter().any(|p| p.name == "source") {
                params.push(IndicatorParam::source("source", "Source"));
            }
        }

        // CVD display mode (owner spec 2026-07-11): "Line" = the sub-pane
        // line (default, unchanged); "Candles" = the sub-pane is hidden and
        // the indicator acts as an overlay body-painter on the main chart —
        // per-bar delta sign recolors candle bodies, gated to
        // `ChartTypeDef::delta_paintable` chart types by the render path.
        // Render-side toggle only: catalog compute is untouched.
        if info.id() == "CVD" {
            params.push(IndicatorParam::select(
                "display",
                "Display",
                vec!["Line".to_string(), "Candles".to_string()],
                0,
            ));
        }

        params
    }

    /// Convert snake_case parameter names to human-readable labels
    /// Examples: "fast_ma_type" -> "Fast MA Type", "slow_source" -> "Slow Source"
    fn humanize_param_name(name: &str) -> String {
        name.split('_')
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => {
                        // Capitalize first letter, keep rest as-is (preserves MA, ATR, etc.)
                        let rest: String = chars.collect();
                        if word.len() <= 3 && word.chars().all(|c| c.is_ascii_uppercase()) {
                            // Keep abbreviations like MA, ATR uppercase
                            word.to_uppercase()
                        } else {
                            format!("{}{}", first.to_uppercase(), rest)
                        }
                    }
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Map catalog category to core category
    fn map_category(cat: CatalogCategory) -> IndicatorCategory {
        match cat {
            CatalogCategory::Momentum => IndicatorCategory::Momentum,
            CatalogCategory::Volatility => IndicatorCategory::Volatility,
            CatalogCategory::Volume => IndicatorCategory::Volume,
            CatalogCategory::Average | CatalogCategory::Trend | CatalogCategory::TrendStop => IndicatorCategory::Trend,
            CatalogCategory::Channels | CatalogCategory::Levels => IndicatorCategory::Trend,
            CatalogCategory::Statistics | CatalogCategory::Regression => IndicatorCategory::Custom,
            CatalogCategory::Entropy | CatalogCategory::Chaos => IndicatorCategory::Custom,
            CatalogCategory::SignalProcessing | CatalogCategory::Kalman => IndicatorCategory::Oscillator,
            CatalogCategory::Adaptive => IndicatorCategory::Trend,
            CatalogCategory::Accumulation | CatalogCategory::Book => IndicatorCategory::Volume,
            CatalogCategory::Candles => IndicatorCategory::Custom,
            CatalogCategory::Clusters => IndicatorCategory::Volume,
            CatalogCategory::Divergence => IndicatorCategory::Oscillator,
            CatalogCategory::Ratio => IndicatorCategory::Custom,
            CatalogCategory::Position => IndicatorCategory::Custom,
            CatalogCategory::Zigzag => IndicatorCategory::Trend,
            // The 6 opaque host-driven overlays (overlay_catalog.rs) —
            // previously fell into the `Custom` catch-all below.
            CatalogCategory::Overlay => IndicatorCategory::TotalOverlay,
            // Catch-all for any new categories
            CatalogCategory::Custom | CatalogCategory::Composite |
            CatalogCategory::Experimental | CatalogCategory::Unknown => IndicatorCategory::Custom,
            // New mli stream-specific categories (FundingAdvanced, OpenInterest, etc.) — render parity TBD in phase 3.
            _ => IndicatorCategory::Custom,
        }
    }

    /// Map output type from catalog to core
    fn map_output_type(ot: CatalogOutputType) -> IndicatorOutputType {
        match ot {
            CatalogOutputType::Line => IndicatorOutputType::Line,
            CatalogOutputType::Histogram => IndicatorOutputType::Histogram,
            CatalogOutputType::Band => IndicatorOutputType::Band,
            CatalogOutputType::Area => IndicatorOutputType::Area,
            CatalogOutputType::Dots => IndicatorOutputType::Dots,
            CatalogOutputType::Background => IndicatorOutputType::Background,
            CatalogOutputType::Cloud => IndicatorOutputType::Band, // Cloud renders as band
        }
    }

    /// Map histogram style from catalog to core
    fn map_histogram_style(hs: CatalogHistogramStyle) -> HistogramStyle {
        match hs {
            CatalogHistogramStyle::FromBottom => HistogramStyle::FromBottom,
            CatalogHistogramStyle::Centered => HistogramStyle::Centered,
            CatalogHistogramStyle::FromTop => HistogramStyle::FromBottom, // No FromTop in core
        }
    }

    /// Map output spec to indicator output
    fn map_output(spec: &OutputSpec) -> IndicatorOutput {
        IndicatorOutput {
            name: spec.name.clone(),
            display_name: spec.display_name.clone(),
            output_type: Self::map_output_type(spec.output_type),
            color: spec.default_color.clone(),
            line_width: spec.default_line_width,
            visible: spec.visible_by_default,
        }
    }

    // =========================================================================
    // Catalog Access
    // =========================================================================

    /// Get all indicator definitions from the catalog
    pub fn get_all_definitions(&self) -> Vec<IndicatorDefinition> {
        self.catalog
            .get_all()
            .iter()
            .map(|info| self.to_definition(info))
            .collect()
    }

    /// Get indicator definition by ID
    pub fn get_definition(&self, id: &str) -> Option<IndicatorDefinition> {
        self.catalog
            .get(id)
            .ok()
            .map(|info| self.to_definition(&info))
    }

    /// Get all overlay indicators — by `IndicatorDefinition::overlay`, the
    /// space `to_definition` derives (signature `metadata("kind")` first),
    /// not mli's own `is_overlay()`, which ignores `kind` and disagrees for
    /// the host-rendered entries (e.g. `overlay_volume_profile`).
    pub fn get_overlay_definitions(&self) -> Vec<IndicatorDefinition> {
        self.get_all_definitions()
            .into_iter()
            .filter(|d| d.overlay)
            .collect()
    }

    /// Get all sub-pane indicators — same source of truth as
    /// [`Self::get_overlay_definitions`].
    pub fn get_subpane_definitions(&self) -> Vec<IndicatorDefinition> {
        self.get_all_definitions()
            .into_iter()
            .filter(|d| !d.overlay)
            .collect()
    }

    /// Get definitions by category
    pub fn get_by_category(&self, category: IndicatorCategory) -> Vec<IndicatorDefinition> {
        self.get_all_definitions()
            .into_iter()
            .filter(|d| d.category == category)
            .collect()
    }

    /// Search for indicators
    pub fn search(&self, query: &str) -> Vec<IndicatorDefinition> {
        self.catalog
            .search(query)
            .iter()
            .map(|info| self.to_definition(info))
            .collect()
    }

    /// Get catalog statistics
    pub fn stats(&self) -> UnifiedCatalogStats {
        self.catalog.stats()
    }

    /// Get total count of indicators
    pub fn total_count(&self) -> usize {
        self.catalog.total_count()
    }

    /// Check if indicator exists
    pub fn contains(&self, id: &str) -> bool {
        self.catalog.contains(id)
    }
}

impl Default for IndicatorBridge {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bridge_creation() {
        let bridge = IndicatorBridge::new();
        assert!(bridge.total_count() > 400, "Should have 400+ indicators");
    }

    #[test]
    fn test_get_rsi_definition() {
        let bridge = IndicatorBridge::new();
        let rsi = bridge.get_definition("RSI").unwrap();

        assert_eq!(rsi.type_id, "rsi");
        assert_eq!(rsi.short_name, "RSI");
        assert!(!rsi.overlay);
        assert_eq!(rsi.bounds, Some((0.0, 100.0)));
    }

    #[test]
    fn test_get_sma_definition() {
        let bridge = IndicatorBridge::new();
        let sma = bridge.get_definition("SMA").unwrap();

        assert_eq!(sma.type_id, "sma");
        assert!(sma.overlay);
        assert_eq!(sma.bounds, None);
    }

    #[test]
    fn test_get_macd_definition() {
        let bridge = IndicatorBridge::new();
        let macd = bridge.get_definition("MACD").unwrap();

        assert_eq!(macd.type_id, "macd");
        assert!(!macd.overlay);
        assert!(macd.zero_baseline);
        assert_eq!(macd.histogram_style, HistogramStyle::Centered);
        assert_eq!(macd.outputs.len(), 3);
    }

    #[test]
    fn test_get_all_definitions() {
        let bridge = IndicatorBridge::new();
        let all = bridge.get_all_definitions();

        assert!(all.len() > 400, "Should have 400+ definitions");
    }

    #[test]
    fn test_volume_definition_registered() {
        // Owner report 2026-07-05: VOLUME neither auto-added nor in the
        // picker. This pins the mli→definitions hop: the catalog VOLUME
        // signature must arrive as an overlay histogram definition.
        let bridge = IndicatorBridge::new();
        let vol = bridge.get_definition("VOLUME").expect("VOLUME must be in the catalog");
        assert_eq!(vol.type_id, "volume");
        assert!(vol.overlay, "VOLUME must render in the main pane");
        assert_eq!(vol.histogram_style, HistogramStyle::FromBottom);
        assert!(
            vol.outputs.iter().any(|o| matches!(o.output_type, IndicatorOutputType::Histogram)),
            "VOLUME output must be a histogram"
        );
        // The picker/definitions map is fed by get_all_definitions (the
        // per-category enumeration), NOT by get_definition's direct id
        // lookup — pin that hop separately.
        let all = bridge.get_all_definitions();
        assert!(
            all.iter().any(|d| d.type_id == "volume"),
            "VOLUME must be present in get_all_definitions (picker source)"
        );
    }

    #[test]
    fn test_get_overlay_definitions() {
        let bridge = IndicatorBridge::new();
        let overlays = bridge.get_overlay_definitions();

        assert!(!overlays.is_empty());
        for def in &overlays {
            assert!(def.overlay, "{} should be overlay", def.type_id);
        }
    }

    #[test]
    fn test_get_subpane_definitions() {
        let bridge = IndicatorBridge::new();
        let subpanes = bridge.get_subpane_definitions();

        assert!(!subpanes.is_empty());
        for def in &subpanes {
            assert!(!def.overlay, "{} should be sub-pane", def.type_id);
        }
    }

    #[test]
    fn test_search() {
        let bridge = IndicatorBridge::new();
        let results = bridge.search("moving average");

        assert!(!results.is_empty());
    }

    #[test]
    fn test_category_mapping() {
        let bridge = IndicatorBridge::new();

        // RSI should be Momentum
        let rsi = bridge.get_definition("RSI").unwrap();
        assert_eq!(rsi.category, IndicatorCategory::Momentum);

        // BB should be Volatility (channels map to Trend but BB is volatility)
        // Actually BB is in Channels category -> maps to Trend
        let bb = bridge.get_definition("BB").unwrap();
        assert_eq!(bb.category, IndicatorCategory::Trend);

        // ATR should be Volatility
        let atr = bridge.get_definition("ATR").unwrap();
        assert_eq!(atr.category, IndicatorCategory::Volatility);
    }

    // ── Arc D step 4: fed_by_engine un-hide (docs/mlc/plans/
    // indicator-streams-arc-d-2026-09-25.md §3.6/§6) ───────────────────────

    /// `fed_by_engine` must equal "every declared stream (primary + aux) is
    /// in `FED_STREAMS`" for EVERY compute indicator (`machine_id:
    /// Some(..)`) — checked both ways, so this catches an entry that
    /// should have un-hidden and didn't, AND an entry that should have
    /// stayed hidden (or gone hidden, for a Bar-primary composite with an
    /// unfed aux stream) and didn't.
    #[test]
    fn fed_by_engine_matches_fed_streams_set_for_every_compute_entry() {
        let bridge = IndicatorBridge::new();
        let all = bridge.get_all_definitions();
        let compute_defs: Vec<_> = all.iter().filter(|d| d.machine_id.is_some()).collect();
        assert!(!compute_defs.is_empty());

        for d in &compute_defs {
            let all_wired = d.streams.iter().all(|s| FED_STREAMS.contains(s));
            assert_eq!(
                d.fed_by_engine, all_wired,
                "{}: fed_by_engine={} but streams={:?} (wired set = {:?})",
                d.type_id, d.fed_by_engine, d.streams, FED_STREAMS,
            );
        }
    }

    /// Pins the exact count of un-hidden entries (Arc D step 4's own ask,
    /// §6 step 4: "pinning the exact count of entries that become
    /// visible" — replaces the plan's hand-waved "~63-67", which covered
    /// steps 5/6 too; after step 3 only `Tick` joined `Bar`). Excludes the
    /// opaque host overlays (`machine_id: None`, always visible regardless
    /// of `streams`) and pure-`Bar` entries (already visible before this
    /// arc) — counts only NEWLY visible non-`Bar`-primary compute entries.
    ///
    /// 10, not the CLAUDE.md stream table's raw "Tick 22" primary count:
    /// 12 of those 22 declare at least one `aux_streams` entry outside
    /// `{Bar, Tick}` (composites needing e.g. `OrderBook`/`Funding`
    /// alongside their `Tick` primary) and correctly stay hidden per the
    /// composite rule (§3.6 case 1) — a live catalog query is the only
    /// correct source for this number, matching this task's own
    /// instruction to verify every plan example against the catalog. The
    /// 10 (verified live, 2026-09-25): `trade_flow_imbalance`,
    /// `uptick_downtick_volume`, `aggressor_imbalance`,
    /// `large_trade_filter`, `tick_volume`, `footprint_chart`,
    /// `footprint_imb`, `footprint_poc`, `absorption_detector`,
    /// `trade_cluster_detector`.
    /// A future step (5 or 6) extending `FED_STREAMS` is expected to move
    /// this number — update it deliberately, not silently.
    #[test]
    fn newly_visible_non_bar_primary_entry_count_is_pinned() {
        use mli::data_loader::stream_kind::StreamKind;
        let bridge = IndicatorBridge::new();
        let all = bridge.get_all_definitions();

        let newly_visible: usize = all.iter()
            .filter(|d| d.machine_id.is_some())
            .filter(|d| d.fed_by_engine)
            .filter(|d| d.streams.first() != Some(&StreamKind::Bar))
            .count();

        assert_eq!(
            newly_visible, 10,
            "newly-visible non-Bar-primary compute entry count drifted from the Arc D step 4 pin (10)"
        );
    }

    /// A representative entry per still-hidden non-`Tick` stream (OrderBook,
    /// Funding, OpenInterest, Liquidation — steps 5/6, not yet landed) stays
    /// hidden; a representative `Tick`-primary entry (step 3, this task) is
    /// now visible. Same indicators `mlc-backend-indicator-engine`'s and
    /// `mlc-backend`'s own step 1-3 tests already exercise, so a name typo
    /// here would already be caught upstream.
    #[test]
    fn representative_entries_per_stream_match_expected_visibility() {
        let bridge = IndicatorBridge::new();

        let tick = bridge.get_definition("TRADE_FLOW_IMBALANCE").expect("TRADE_FLOW_IMBALANCE in catalog");
        assert!(tick.fed_by_engine, "Tick-primary entry must be visible after step 3's Tick hook");

        for hidden_id in ["BOOK_IMB", "FUNDING_MOMENTUM", "OI_CHANGE_RATE", "LIQUIDATION_RATE"] {
            let def = bridge.get_definition(hidden_id).unwrap_or_else(|| panic!("{hidden_id} in catalog"));
            assert!(
                !def.fed_by_engine,
                "{hidden_id} declares a stream steps 5/6 haven't landed yet — must stay hidden"
            );
        }
    }
}
