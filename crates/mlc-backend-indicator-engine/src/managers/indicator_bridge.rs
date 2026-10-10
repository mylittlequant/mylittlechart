//! Indicator bridge.
//!
//! Compute rows come from `ContractFactory` plus `Render::rendering`.
//! Host layers come from `HostOverlayId`. There is no signature table.

use mli::contract::{
    Family, HistogramStyle as RenderHistogram, HostOverlayId, OutputType as RenderOutputType,
    PaintSpace,
};
use mli::engine::contract_engine::{
    family_of, inputs_of, output_ids_of, rendering_of, source_of, ContractFactory,
};
use mli::engine::indicator_id::IndicatorId;
use mli::engine::stream_kind::StreamKind;

use super::contract_instance::ContractInstance;
use super::indicator_manager::{
    HistogramStyle, IndicatorCategory, IndicatorDefinition, IndicatorOutput, IndicatorOutputType,
    IndicatorParam,
};

/// Streams a live factory actually receives today.
const FED_STREAMS: &[StreamKind] = &[
    StreamKind::Bar,
    StreamKind::Tick,
    StreamKind::OrderBook,
    StreamKind::Liquidation,
    StreamKind::Funding,
    StreamKind::OpenInterest,
];

fn all_streams_fed(streams: &[StreamKind]) -> bool {
    streams.iter().all(|s| FED_STREAMS.contains(s))
}

/// Catalog projection the chart picker and the instance factory share.
pub struct IndicatorBridge {
    definitions: Vec<IndicatorDefinition>,
}

impl IndicatorBridge {
    pub fn new() -> Self {
        let periods = ContractInstance::primary_periods();
        let mut definitions = Vec::new();
        for id in IndicatorId::all() {
            if !ContractFactory::is_migrated(id) {
                continue;
            }
            definitions.push(Self::compute_definition(id, periods.get(&id).copied().flatten()));
        }
        for id in HostOverlayId::ALL {
            definitions.push(Self::host_definition(id));
        }
        Self { definitions }
    }

    fn compute_definition(id: IndicatorId, period: Option<usize>) -> IndicatorDefinition {
        let rendering = rendering_of(id);
        let streams: Vec<StreamKind> = inputs_of(id).unwrap_or(&[]).to_vec();
        let family = family_of(id).and_then(|f| f.first().copied());
        let outputs = match &rendering {
            Some(spec) => spec
                .outputs
                .iter()
                .map(|out| IndicatorOutput {
                    name: out.id.name().to_string(),
                    display_name: out.label.to_string(),
                    output_type: map_output_type(out.kind),
                    color: out.color.to_hex(),
                    line_width: out.line_width,
                    visible: out.visible,
                })
                .collect(),
            None => vec![IndicatorOutput::line(
                output_ids_of(id)
                    .first()
                    .map(|o| o.name())
                    .unwrap_or("value"),
                &id.label(),
                "#2196F3",
            )],
        };
        let mut params = Vec::new();
        if let Some(period) = period {
            params.push(IndicatorParam::int("period", "Period", period as i32, 1, 10_000));
        }
        if source_of(id).is_some() {
            params.push(IndicatorParam::source("source", "Source"));
        }
        if id == IndicatorId::Cvd {
            params.push(IndicatorParam::select(
                "display",
                "Display",
                vec!["Line".to_string(), "Candles".to_string()],
                0,
            ));
        }
        let type_id = id.type_id();
        let label = id.label();
        IndicatorDefinition {
            type_id: type_id.clone(),
            name: label.clone(),
            short_name: label,
            description: family_phrase(family).to_string(),
            category: map_family(family),
            params,
            outputs,
            overlay: rendering.as_ref().map(|r| r.overlay).unwrap_or(false),
            precision: rendering.as_ref().map(|r| r.precision).unwrap_or(4),
            bounds: rendering.as_ref().and_then(|r| r.bounds),
            zero_baseline: rendering.as_ref().map(|r| r.zero_baseline).unwrap_or(false),
            histogram_style: rendering
                .as_ref()
                .map(|r| map_histogram(r.histogram_style))
                .unwrap_or(HistogramStyle::FromBottom),
            machine_id: Some(id),
            streams,
            fed_by_engine: all_streams_fed(inputs_of(id).unwrap_or(&[])),
            is_pattern_role: false,
        }
    }

    fn host_definition(id: HostOverlayId) -> IndicatorDefinition {
        let streams = id.stream().into_iter().collect();
        IndicatorDefinition {
            type_id: id.as_str().to_string(),
            name: id.name().to_string(),
            short_name: id.name().to_string(),
            description: String::new(),
            category: IndicatorCategory::TotalOverlay,
            params: Vec::new(),
            outputs: Vec::new(),
            overlay: id.paint() == PaintSpace::OnPrice,
            precision: 2,
            bounds: None,
            zero_baseline: false,
            histogram_style: HistogramStyle::FromBottom,
            machine_id: None,
            streams,
            fed_by_engine: true,
            is_pattern_role: false,
        }
    }

    pub fn get_all_definitions(&self) -> Vec<IndicatorDefinition> {
        self.definitions.clone()
    }

    pub fn get_definition(&self, id: &str) -> Option<IndicatorDefinition> {
        let want = id.to_ascii_lowercase();
        self.definitions.iter().find(|d| d.type_id == want).cloned()
    }

    pub fn get_overlay_definitions(&self) -> Vec<IndicatorDefinition> {
        self.definitions.iter().filter(|d| d.overlay).cloned().collect()
    }

    pub fn get_subpane_definitions(&self) -> Vec<IndicatorDefinition> {
        self.definitions.iter().filter(|d| !d.overlay).cloned().collect()
    }

    pub fn get_by_category(&self, category: IndicatorCategory) -> Vec<IndicatorDefinition> {
        self.definitions
            .iter()
            .filter(|d| d.category == category)
            .cloned()
            .collect()
    }

    pub fn search(&self, query: &str) -> Vec<IndicatorDefinition> {
        let q = query.to_ascii_lowercase();
        self.definitions
            .iter()
            .filter(|d| {
                d.type_id.contains(&q)
                    || d.short_name.to_ascii_lowercase().contains(&q)
                    || d.name.to_ascii_lowercase().contains(&q)
                    || d.description.to_ascii_lowercase().contains(&q)
            })
            .cloned()
            .collect()
    }

    pub fn total_count(&self) -> usize {
        self.definitions.len()
    }

    pub fn contains(&self, id: &str) -> bool {
        self.get_definition(id).is_some()
    }
}

impl Default for IndicatorBridge {
    fn default() -> Self {
        Self::new()
    }
}

fn map_family(family: Option<Family>) -> IndicatorCategory {
    match family {
        Some(Family::MovingAverage | Family::Trend | Family::Channel) => IndicatorCategory::Trend,
        Some(Family::Oscillator) => IndicatorCategory::Momentum,
        Some(Family::Volatility) => IndicatorCategory::Volatility,
        Some(Family::OrderBook | Family::Liquidations) => IndicatorCategory::Volume,
        Some(Family::OpenInterest) | None => IndicatorCategory::Custom,
    }
}

fn map_histogram(style: RenderHistogram) -> HistogramStyle {
    match style {
        RenderHistogram::Centered => HistogramStyle::Centered,
        RenderHistogram::FromBottom | RenderHistogram::FromTop => HistogramStyle::FromBottom,
    }
}

fn map_output_type(kind: RenderOutputType) -> IndicatorOutputType {
    match kind {
        RenderOutputType::Line => IndicatorOutputType::Line,
        RenderOutputType::Histogram => IndicatorOutputType::Histogram,
        RenderOutputType::Band | RenderOutputType::Cloud => IndicatorOutputType::Band,
        RenderOutputType::Area => IndicatorOutputType::Area,
        RenderOutputType::Dots => IndicatorOutputType::Dots,
        RenderOutputType::Background => IndicatorOutputType::Background,
    }
}

fn family_phrase(family: Option<Family>) -> &'static str {
    match family {
        Some(Family::MovingAverage) => "moving average",
        Some(Family::Oscillator) => "oscillator momentum",
        Some(Family::Channel) => "channel",
        Some(Family::Volatility) => "volatility",
        Some(Family::Trend) => "trend",
        Some(Family::OrderBook) => "order book",
        Some(Family::OpenInterest) => "open interest",
        Some(Family::Liquidations) => "liquidation",
        None => "",
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
        let bridge = IndicatorBridge::new();
        let vol = bridge.get_definition("VOLUME").expect("VOLUME must be in the catalog");
        assert_eq!(vol.type_id, "volume");
        assert!(vol.overlay, "VOLUME must render in the main pane");
        assert_eq!(vol.histogram_style, HistogramStyle::FromBottom);
        assert!(
            vol.outputs.iter().any(|o| matches!(o.output_type, IndicatorOutputType::Histogram)),
            "VOLUME output must be a histogram"
        );
        let all = bridge.get_all_definitions();
        assert!(all.iter().any(|d| d.type_id == "volume"));
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
        let rsi = bridge.get_definition("RSI").unwrap();
        assert_eq!(rsi.category, IndicatorCategory::Momentum);
        let bb = bridge.get_definition("BB").unwrap();
        assert_eq!(bb.category, IndicatorCategory::Trend);
        let atr = bridge.get_definition("ATR").unwrap();
        assert_eq!(atr.category, IndicatorCategory::Volatility);
    }

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

    /// The old `get_all` cutoff hid Liquidations, Funding, and Open interest.
    /// The factory walk includes them.
    #[test]
    fn stream_categories_are_reachable() {
        let bridge = IndicatorBridge::new();
        let all = bridge.get_all_definitions();
        let reachable = |want: StreamKind| -> usize {
            all.iter()
                .filter(|d| d.machine_id.is_some())
                .filter(|d| d.streams.contains(&want))
                .count()
        };
        assert!(reachable(StreamKind::OrderBook) > 0);
        assert!(reachable(StreamKind::Liquidation) > 0);
        assert!(reachable(StreamKind::Funding) > 0);
        assert!(reachable(StreamKind::OpenInterest) > 0);
        assert!(
            all.iter().any(|d| d.type_id == "overlay_dom_heatmap"),
            "host layers come from HostOverlayId::ALL"
        );
    }

    #[test]
    fn representative_entries_per_stream_match_expected_visibility() {
        let bridge = IndicatorBridge::new();
        let tick = bridge
            .get_definition("TRADE_FLOW_IMBALANCE")
            .expect("TRADE_FLOW_IMBALANCE in catalog");
        assert!(tick.fed_by_engine);
        for visible_id in ["BOOK_IMB", "LIQUIDATION_RATE", "FUNDING_MOMENTUM", "OI_CHANGE_RATE"] {
            let def = bridge
                .get_definition(visible_id)
                .unwrap_or_else(|| panic!("{visible_id} in catalog"));
            assert!(def.fed_by_engine, "{visible_id} must be visible");
        }
    }
}
