//! Indicator Bridge
//!
//! Converts indicators from the zengeld-chart-indicators catalog to
//! indicator definitions for rendering.

use crate::catalog::{
    UnifiedIndicatorCatalog, UnifiedIndicatorInfo, UnifiedCatalogStats,
    OutputSpec, OutputType as CatalogOutputType,
    HistogramStyle as CatalogHistogramStyle,
    IndicatorCategory as CatalogCategory,
    ParamType as CatalogParamType,
};

use super::indicator_manager::{
    IndicatorDefinition, IndicatorCategory, IndicatorOutput, IndicatorOutputType,
    IndicatorParam, HistogramStyle,
};

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
        let overlay = rendering.map(|r| r.overlay).unwrap_or(false);
        let bounds = rendering.and_then(|r| r.bounds);
        let zero_baseline = rendering.map(|r| r.zero_baseline).unwrap_or(false);
        let histogram_style = rendering
            .map(|r| Self::map_histogram_style(r.histogram_style))
            .unwrap_or(HistogramStyle::FromBottom);
        let precision = rendering.map(|r| r.precision).unwrap_or(4);

        // Get description
        let description = info.signature.description.clone();

        IndicatorDefinition {
            type_id: info.id().to_lowercase(),
            name: info.signature.name.clone(),
            short_name: info.id().to_string(),
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
                _ => {
                    // Skip other types (String, etc.)
                }
            }
        }

        // Add global source parameter ONLY for PriceOnly indicators (backward compatibility)
        // This is the main "source" parameter that applies to the whole indicator
        use crate::catalog::SourceType;
        if info.signature.source_type == SourceType::PriceOnly {
            // Only add if there's no component-level source already
            if !params.iter().any(|p| p.name == "source") {
                params.push(IndicatorParam::source("source", "Source"));
            }
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
            // Catch-all for any new categories
            CatalogCategory::Custom | CatalogCategory::Composite |
            CatalogCategory::Experimental | CatalogCategory::Unknown => IndicatorCategory::Custom,
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

    /// Get all overlay indicators
    pub fn get_overlay_definitions(&self) -> Vec<IndicatorDefinition> {
        self.catalog
            .get_overlay_indicators()
            .iter()
            .map(|info| self.to_definition(info))
            .collect()
    }

    /// Get all sub-pane indicators
    pub fn get_subpane_definitions(&self) -> Vec<IndicatorDefinition> {
        self.catalog
            .get_subpane_indicators()
            .iter()
            .map(|info| self.to_definition(info))
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
}
