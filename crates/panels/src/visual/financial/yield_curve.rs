use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct YieldCurveId(pub u64);

#[derive(Clone, Debug, Default)]
pub struct YieldCurveState {
    pub curves: HashMap<String, YieldCurve>,  // "US_current", "US_1y_ago", etc.
    pub selected_curves: Vec<String>,
    pub date_range: (i64, i64),  // Unix timestamps
    pub highlight_curve: Option<String>,
}

#[derive(Clone, Debug)]
pub struct YieldCurve {
    pub label: String,
    pub date: i64,  // Unix timestamp
    pub points: Vec<(f64, f64)>,  // (maturity_years, yield_percent)
    pub color: [f32; 3],  // OKLCH
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct YieldCurveConfig {
    pub maturities: Vec<f64>,  // [0.25, 0.5, 1, 2, 5, 10, 20, 30] years
    pub y_axis_range: Option<(f64, f64)>,  // Auto if None
    pub show_inversion_zones: bool,  // Highlight inverted sections
    pub line_width: f32,
    pub show_points: bool,
    pub interpolation: CurveInterpolation,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CurveInterpolation {
    Linear,
    CubicSpline,
}

impl YieldCurveState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns curve points in screen coordinates for the given curve name
    pub fn curve_points(&self, curve_name: &str, w: f32, h: f32) -> Vec<(f32, f32)> {
        let Some(curve) = self.curves.get(curve_name) else {
            return Vec::new();
        };

        if curve.points.is_empty() {
            return Vec::new();
        }

        let (min_maturity, max_maturity) = curve.points.iter()
            .fold((f64::MAX, f64::MIN), |(min, max), (mat, _)| {
                (min.min(*mat), max.max(*mat))
            });

        let (min_yield, max_yield) = curve.points.iter()
            .fold((f64::MAX, f64::MIN), |(min, max), (_, yld)| {
                (min.min(*yld), max.max(*yld))
            });

        if max_maturity <= min_maturity || max_yield <= min_yield {
            return Vec::new();
        }

        curve.points.iter()
            .map(|(maturity, yld)| {
                let x = (((maturity - min_maturity) / (max_maturity - min_maturity)) as f32 * w).max(0.0).min(w);
                let y = (h - ((yld - min_yield) / (max_yield - min_yield)) as f32 * h).max(0.0).min(h);
                (x, y)
            })
            .collect()
    }

    /// Returns the yield spread compared to the previous point on the curve
    pub fn spread_to_previous(&self, curve_name: &str, maturity: f64) -> Option<f64> {
        let curve = self.curves.get(curve_name)?;
        let idx = curve.points.iter().position(|(mat, _)| *mat == maturity)?;
        if idx == 0 {
            return None;
        }
        let current_yield = curve.points[idx].1;
        let prev_yield = curve.points[idx - 1].1;
        Some(current_yield - prev_yield)
    }

    /// Formats a yield value as a percentage string
    pub fn format_yield(&self, yield_value: f64) -> String {
        format!("{:.2}%", yield_value)
    }
}

impl Default for YieldCurveConfig {
    fn default() -> Self {
        Self {
            maturities: vec![0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 30.0],
            y_axis_range: None,
            show_inversion_zones: true,
            line_width: 2.0,
            show_points: true,
            interpolation: CurveInterpolation::Linear,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct YieldCurvePanel {
    id: YieldCurveId,
    title: String,
}

impl YieldCurvePanel {
    pub fn new(id: YieldCurveId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> YieldCurveId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "yield_curve"
    }

    pub fn kind_label(&self) -> &'static str {
        "Yield Curve"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 300.0)
    }
}
