//! Fibonacci Wedge primitive
//!
//! A wedge/triangle shape with Fibonacci levels inside.
//! Three points define the wedge, levels are drawn between the sides.

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata,
    ControlPoint, ControlPointType,
    PrimitiveColor, LineStyle, TextAlign,
    HIT_TOLERANCE, CONTROL_POINT_RADIUS,
    RenderContext, crisp, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    render_primitive_text_rotated, calculate_line_text_params,
    config::FibLevelConfig,
};
use super::retracement::default_level_configs;

/// Fibonacci Wedge
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FibWedge {
    /// Common primitive data
    pub data: PrimitiveData,
    /// Apex bar (point 1 - the tip)
    pub bar1: f64,
    /// Apex price
    pub price1: f64,
    /// Upper corner bar (point 2)
    pub bar2: f64,
    /// Upper corner price
    pub price2: f64,
    /// Lower corner bar (point 3)
    pub bar3: f64,
    /// Lower corner price
    pub price3: f64,
    /// Fibonacci level configurations (with individual colors/widths)
    #[serde(default = "default_level_configs", deserialize_with = "deserialize_level_configs")]
    pub level_configs: Vec<FibLevelConfig>,
    /// Show labels
    #[serde(default = "default_true")]
    pub show_labels: bool,
    /// Show percentage/level labels
    #[serde(default = "default_true")]
    pub show_percentages: bool,
    /// Label position: "left", "right", "center"
    #[serde(default = "default_label_position")]
    pub label_position: String,
    /// Show levels as percentages (true) or coefficients (false)
    #[serde(default = "default_true")]
    pub show_as_percent: bool,
    /// Fill the wedge
    #[serde(default)]
    pub fill: bool,
    /// Fill opacity
    #[serde(default = "default_fill_opacity")]
    pub fill_opacity: f64,
}

fn default_true() -> bool { true }
fn default_fill_opacity() -> f64 { 0.1 }
fn default_label_position() -> String { "left".to_string() }

/// Backward compatibility: deserialize old `levels: Vec<f64>` format
fn deserialize_level_configs<'de, D>(deserializer: D) -> Result<Vec<FibLevelConfig>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum LevelFormat {
        Configs(Vec<FibLevelConfig>),
        Levels(Vec<f64>),
    }

    match LevelFormat::deserialize(deserializer)? {
        LevelFormat::Configs(configs) => Ok(configs),
        LevelFormat::Levels(levels) => {
            // Convert old format to new format
            Ok(levels.iter().map(|&level| FibLevelConfig::new(level)).collect())
        }
    }
}

impl FibWedge {
    /// Create a new Fibonacci wedge
    pub fn new(bar1: f64, price1: f64, bar2: f64, price2: f64, bar3: f64, price3: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "fib_wedge".to_string(),
                display_name: "Fib Wedge".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            bar1,
            price1,
            bar2,
            price2,
            bar3,
            price3,
            level_configs: default_level_configs(),
            show_labels: true,
            show_percentages: true,
            label_position: "left".to_string(),
            show_as_percent: true,
            fill: false,
            fill_opacity: 0.1,
        }
    }

    /// Get a point on the upper edge at parameter t (0=apex, 1=corner)
    fn upper_edge_point(&self, t: f64) -> (f64, f64) {
        (
            self.bar1 + t * (self.bar2 - self.bar1),
            self.price1 + t * (self.price2 - self.price1),
        )
    }

    /// Get a point on the lower edge at parameter t (0=apex, 1=corner)
    fn lower_edge_point(&self, t: f64) -> (f64, f64) {
        (
            self.bar1 + t * (self.bar3 - self.bar1),
            self.price1 + t * (self.price3 - self.price1),
        )
    }
}

impl Primitive for FibWedge {
    fn type_id(&self) -> &'static str {
        "fib_wedge"
    }

    fn display_name(&self) -> &str {
        &self.data.display_name
    }

    fn kind(&self) -> PrimitiveKind {
        PrimitiveKind::Fibonacci
    }

    fn click_behavior(&self) -> ClickBehavior {
        ClickBehavior::ThreePoint
    }

    fn data(&self) -> &PrimitiveData {
        &self.data
    }

    fn data_mut(&mut self) -> &mut PrimitiveData {
        &mut self.data
    }

    fn points(&self) -> Vec<(f64, f64)> {
        vec![
            (self.bar1, self.price1),
            (self.bar2, self.price2),
            (self.bar3, self.price3),
        ]
    }

    fn set_points(&mut self, points: &[(f64, f64)]) {
        if let Some(&(bar, price)) = points.first() {
            self.bar1 = bar;
            self.price1 = price;
        }
        if let Some(&(bar, price)) = points.get(1) {
            self.bar2 = bar;
            self.price2 = price;
        }
        if let Some(&(bar, price)) = points.get(2) {
            self.bar3 = bar;
            self.price3 = price;
        }
    }

    fn translate(&mut self, bar_delta: f64, price_delta: f64) {
        self.bar1 += bar_delta;
        self.bar2 += bar_delta;
        self.bar3 += bar_delta;
        self.price1 += price_delta;
        self.price2 += price_delta;
        self.price3 += price_delta;
    }

    fn move_control_point(&mut self, point_type: ControlPointType, bar: f64, price: f64) {
        match point_type {
            ControlPointType::Point1 => {
                self.bar1 = bar;
                self.price1 = price;
            }
            ControlPointType::Point2 => {
                self.bar2 = bar;
                self.price2 = price;
            }
            ControlPointType::Point3 => {
                self.bar3 = bar;
                self.price3 = price;
            }
            ControlPointType::Move => {
                let bar_delta = bar - self.bar1;
                let price_delta = price - self.price1;
                self.translate(bar_delta, price_delta);
            }
            _ => {}
        }
    }

    fn hit_test(
        &self,
        screen_x: f64,
        screen_y: f64,
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> HitTestResult {
        let x1 = viewport.bar_to_x_f64(self.bar1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(self.bar2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);
        let x3 = viewport.bar_to_x_f64(self.bar3);
        let y3 = viewport.price_to_y(self.price3, price_scale.price_min, price_scale.price_max);

        // Check control points
        if check_point_hit(screen_x, screen_y, x1, y1) {
            return HitTestResult::ControlPoint(ControlPointType::Point1);
        }
        if check_point_hit(screen_x, screen_y, x2, y2) {
            return HitTestResult::ControlPoint(ControlPointType::Point2);
        }
        if check_point_hit(screen_x, screen_y, x3, y3) {
            return HitTestResult::ControlPoint(ControlPointType::Point3);
        }

        // Check wedge edges
        if point_to_line_distance(screen_x, screen_y, x1, y1, x2, y2) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }
        if point_to_line_distance(screen_x, screen_y, x1, y1, x3, y3) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }
        if point_to_line_distance(screen_x, screen_y, x2, y2, x3, y3) < HIT_TOLERANCE {
            return HitTestResult::Body;
        }

        // Check Fibonacci level lines inside wedge (only visible ones)
        for cfg in &self.level_configs {
            if !cfg.visible {
                continue;
            }

            let (u_bar, u_price) = self.upper_edge_point(cfg.level);
            let (l_bar, l_price) = self.lower_edge_point(cfg.level);

            let ux = viewport.bar_to_x_f64(u_bar);
            let uy = viewport.price_to_y(u_price, price_scale.price_min, price_scale.price_max);
            let lx = viewport.bar_to_x_f64(l_bar);
            let ly = viewport.price_to_y(l_price, price_scale.price_min, price_scale.price_max);

            if point_to_line_distance(screen_x, screen_y, ux, uy, lx, ly) < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
        }

        HitTestResult::Miss
    }

    fn control_points(
        &self,
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Vec<ControlPoint> {
        let x1 = viewport.bar_to_x_f64(self.bar1);
        let y1 = viewport.price_to_y(self.price1, price_scale.price_min, price_scale.price_max);
        let x2 = viewport.bar_to_x_f64(self.bar2);
        let y2 = viewport.price_to_y(self.price2, price_scale.price_min, price_scale.price_max);
        let x3 = viewport.bar_to_x_f64(self.bar3);
        let y3 = viewport.price_to_y(self.price3, price_scale.price_min, price_scale.price_max);

        vec![
            ControlPoint::point1(x1, y1),
            ControlPoint::point2(x2, y2),
            ControlPoint::point3(x3, y3),
        ]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.bar_to_x(self.bar1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.bar_to_x(self.bar2);
        let y2 = ctx.price_to_y(self.price2);
        let x3 = ctx.bar_to_x(self.bar3);
        let y3 = ctx.price_to_y(self.price3);

        // Fill if enabled
        if self.fill {
            let opacity_hex = format!("{:02X}", (self.fill_opacity * 255.0) as u8);
            let fill_color = format!("{}{}", &self.data.color.stroke, opacity_hex);
            ctx.set_fill_color(&fill_color);
            ctx.begin_path();
            ctx.move_to(x1, y1);
            ctx.line_to(x2, y2);
            ctx.line_to(x3, y3);
            ctx.close_path();
            ctx.fill();
        }

        ctx.set_stroke_color(&self.data.color.stroke);
        ctx.set_stroke_width(self.data.width);
        match self.data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        // Draw wedge outline
        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
        ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
        ctx.line_to(crisp(x3, dpr), crisp(y3, dpr));
        ctx.close_path();
        ctx.stroke();

        // Check if we need line gap on median (0.5 level) for v_align == Center
        let needs_gap = self.data.text.as_ref()
            .map(|t| !t.content.is_empty() && matches!(t.v_align, TextAlign::Center))
            .unwrap_or(false);

        // === FILL RENDERING (before level lines so lines are on top) ===
        // Collect visible levels sorted by level value for fill rendering
        let mut visible_levels: Vec<(usize, f64, f64, f64, f64, f64)> = self.level_configs
            .iter()
            .enumerate()
            .filter(|(_, cfg)| cfg.visible)
            .map(|(idx, cfg)| {
                let (u_bar, u_price) = self.upper_edge_point(cfg.level);
                let (l_bar, l_price) = self.lower_edge_point(cfg.level);
                let ux = ctx.bar_to_x(u_bar);
                let uy = ctx.price_to_y(u_price);
                let lx = ctx.bar_to_x(l_bar);
                let ly = ctx.price_to_y(l_price);
                (idx, cfg.level, ux, uy, lx, ly)
            })
            .collect();
        visible_levels.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        // Draw fills between adjacent visible level lines (quads between levels)
        for i in 0..visible_levels.len().saturating_sub(1) {
            let (idx, _, ux1, uy1, lx1, ly1) = visible_levels[i];
            let (_, _, ux2, uy2, lx2, ly2) = visible_levels[i + 1];

            let cfg = &self.level_configs[idx];
            if cfg.fill_enabled {
                let fill_color = cfg.fill_color.as_deref()
                    .or(cfg.color.as_deref())
                    .unwrap_or(&self.data.color.stroke);

                // Draw fill quad between two level lines
                ctx.set_fill_color_alpha(fill_color, cfg.fill_opacity);
                ctx.begin_path();
                ctx.move_to(ux1, uy1);
                ctx.line_to(ux2, uy2);
                ctx.line_to(lx2, ly2);
                ctx.line_to(lx1, ly1);
                ctx.close_path();
                ctx.fill();
                ctx.reset_alpha();
            }
        }

        // Draw Fibonacci level lines inside wedge (only visible ones)
        for cfg in &self.level_configs {
            if !cfg.visible {
                continue;
            }

            let (u_bar, u_price) = self.upper_edge_point(cfg.level);
            let (l_bar, l_price) = self.lower_edge_point(cfg.level);

            let ux = ctx.bar_to_x(u_bar);
            let uy = ctx.price_to_y(u_price);
            let lx = ctx.bar_to_x(l_bar);
            let ly = ctx.price_to_y(l_price);

            // Use level-specific color or fall back to main color
            let color = cfg.color.as_deref().unwrap_or(&self.data.color.stroke);
            ctx.set_stroke_color(color);

            // Use level-specific width or fall back to main width
            let width = cfg.width.unwrap_or(self.data.width);
            ctx.set_stroke_width(width);

            // Parse style from string
            let line_style = match cfg.style.as_str() {
                "dashed" => LineStyle::Dashed,
                "dotted" => LineStyle::Dotted,
                "large_dashed" => LineStyle::LargeDashed,
                "sparse_dotted" => LineStyle::SparseDotted,
                _ => self.data.style.clone(),
            };

            match line_style {
                LineStyle::Solid => ctx.set_line_dash(&[]),
                LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
                LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
                LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
                LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
            }

            let dx = lx - ux;
            let dy = ly - uy;
            let line_len = (dx * dx + dy * dy).sqrt();

            // Build label and calculate gap for this level
            let (label, label_gap_t_start, label_gap_t_end) = if self.show_labels || self.show_percentages {
                let label = {
                    let mut label_parts = Vec::new();
                    if self.show_percentages {
                        if self.show_as_percent {
                            let pct = cfg.level * 100.0;
                            if (pct - pct.round()).abs() < 0.01 {
                                label_parts.push(format!("{}%", pct as i32));
                            } else {
                                label_parts.push(format!("{:.1}%", pct));
                            }
                        } else {
                            let lvl = cfg.level;
                            if (lvl - lvl.round()).abs() < 0.0001 {
                                label_parts.push(format!("{}", lvl as i32));
                            } else if (lvl * 10.0 - (lvl * 10.0).round()).abs() < 0.001 {
                                label_parts.push(format!("{:.1}", lvl));
                            } else {
                                label_parts.push(format!("{:.3}", lvl));
                            }
                        }
                    }
                    label_parts.join(" ")
                };

                if !label.is_empty() && line_len > 0.001 {
                    let char_width = 6.5;
                    let text_width = label.len() as f64 * char_width;
                    let half_gap_t = (text_width / 2.0) / line_len;

                    let (gap_start, gap_end) = match self.label_position.as_str() {
                        "right" => ((1.0 - half_gap_t * 2.0).max(0.0), 1.0),
                        "center" => ((0.5 - half_gap_t).max(0.0), (0.5 + half_gap_t).min(1.0)),
                        _ => (0.0, (half_gap_t * 2.0).min(1.0)), // "left"
                    };
                    (Some(label), gap_start, gap_end)
                } else {
                    (if label.is_empty() { None } else { Some(label) }, 0.0, 0.0)
                }
            } else {
                (None, 0.0, 0.0)
            };

            // Check if this is the median line (0.5 level) and needs text gap
            let is_median = (cfg.level - 0.5).abs() < 0.001;
            let has_label_gap = label.is_some() && label_gap_t_end > label_gap_t_start;

            // Determine gap parameters (label gap takes priority, then text gap for median)
            let (use_gap, gap_t_start, gap_t_end) = if has_label_gap {
                (true, label_gap_t_start, label_gap_t_end)
            } else if is_median && needs_gap && line_len > 0.001 {
                let text = self.data.text.as_ref().unwrap();
                let t_center = match text.h_align {
                    TextAlign::Start => 0.0,
                    TextAlign::Center => 0.5,
                    TextAlign::End => 1.0,
                };
                let char_count = text.content.len() as f64;
                let text_width = char_count * text.font_size * 0.6 + 8.0;
                let half_gap_t = (text_width / 2.0) / line_len;
                (true, (t_center - half_gap_t).max(0.0), (t_center + half_gap_t).min(1.0))
            } else {
                (false, 0.0, 0.0)
            };

            ctx.begin_path();
            if use_gap && gap_t_end > gap_t_start {
                // Draw first segment before gap
                if gap_t_start > 0.001 {
                    let gap_x1 = ux + dx * gap_t_start;
                    let gap_y1 = uy + dy * gap_t_start;
                    ctx.move_to(crisp(ux, dpr), crisp(uy, dpr));
                    ctx.line_to(crisp(gap_x1, dpr), crisp(gap_y1, dpr));
                }

                // Draw second segment after gap
                if gap_t_end < 0.999 {
                    let gap_x2 = ux + dx * gap_t_end;
                    let gap_y2 = uy + dy * gap_t_end;
                    ctx.move_to(crisp(gap_x2, dpr), crisp(gap_y2, dpr));
                    ctx.line_to(crisp(lx, dpr), crisp(ly, dpr));
                }
            } else {
                ctx.move_to(crisp(ux, dpr), crisp(uy, dpr));
                ctx.line_to(crisp(lx, dpr), crisp(ly, dpr));
            }
            ctx.stroke();

            // Draw label in the gap
            if let Some(lbl) = label {
                ctx.set_font("11px sans-serif");
                ctx.set_text_baseline(crate::render::TextBaseline::Middle);
                ctx.set_fill_color(color);

                match self.label_position.as_str() {
                    "right" => {
                        ctx.set_text_align(crate::render::TextAlign::Right);
                        ctx.fill_text(&lbl, lx, ly);
                    }
                    "center" => {
                        ctx.set_text_align(crate::render::TextAlign::Center);
                        ctx.fill_text(&lbl, (ux + lx) / 2.0, (uy + ly) / 2.0);
                    }
                    _ => { // "left"
                        ctx.set_text_align(crate::render::TextAlign::Left);
                        ctx.fill_text(&lbl, ux, uy);
                    }
                }
            }
        }
        ctx.set_line_dash(&[]);

        // Render text if present (positioned based on v_align)
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // Calculate text position based on v_align:
                // - Start: above upper edge (line from apex to point 2)
                // - Center: on median (0.5 level) line
                // - End: below lower edge (line from apex to point 3)
                let (text_start_x, text_start_y, text_end_x, text_end_y) = match text.v_align {
                    TextAlign::Start => {
                        // Above upper edge - use point 2 endpoint at level 1.0
                        let (u_bar, u_price) = self.upper_edge_point(1.0);
                        let ux = ctx.bar_to_x(u_bar);
                        let uy = ctx.price_to_y(u_price);
                        (x1, y1, ux, uy)
                    }
                    TextAlign::Center => {
                        // On median (0.5 level) line
                        let (u_bar, u_price) = self.upper_edge_point(0.5);
                        let (l_bar, l_price) = self.lower_edge_point(0.5);
                        let ux = ctx.bar_to_x(u_bar);
                        let uy = ctx.price_to_y(u_price);
                        let lx = ctx.bar_to_x(l_bar);
                        let ly = ctx.price_to_y(l_price);
                        (ux, uy, lx, ly)
                    }
                    TextAlign::End => {
                        // Below lower edge - use point 3 endpoint at level 1.0
                        let (l_bar, l_price) = self.lower_edge_point(1.0);
                        let lx = ctx.bar_to_x(l_bar);
                        let ly = ctx.price_to_y(l_price);
                        (x1, y1, lx, ly)
                    }
                };

                let params = calculate_line_text_params(text_start_x, text_start_y, text_end_x, text_end_y, text);
                render_primitive_text_rotated(ctx, text, params.x, params.y, &self.data.color.stroke, params.rotation);
            }
        }

        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);

            for (px, py) in [(x1, y1), (x2, y2), (x3, y3)] {
                ctx.begin_path();
                ctx.arc(px, py, CONTROL_POINT_RADIUS as f64, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.stroke();
            }
        }
    }

    fn level_configs(&self) -> Option<Vec<FibLevelConfig>> {
        Some(self.level_configs.clone())
    }

    fn set_level_configs(&mut self, configs: Vec<FibLevelConfig>) -> bool {
        self.level_configs = configs;
        true
    }

    fn style_properties(&self) -> Vec<super::super::config::ConfigProperty> {
        use super::super::config::ConfigProperty;
        vec![
            ConfigProperty::show_labels(self.show_labels).with_order(10),
            ConfigProperty::levels(self.show_percentages).with_order(11),
            ConfigProperty::show_as_percent(self.show_as_percent).with_order(12),
            ConfigProperty::label_position(&self.label_position).with_order(13),
            ConfigProperty::fill(self.fill).with_order(20),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &super::super::config::PropertyValue) -> bool {
        use super::super::config::PropertyValue;
        match id {
            "show_labels" => {
                if let PropertyValue::Boolean(v) = value {
                    self.show_labels = *v;
                    return true;
                }
            }
            "show_percentages" => {
                if let PropertyValue::Boolean(v) = value {
                    self.show_percentages = *v;
                    return true;
                }
            }
            "show_as_percent" => {
                if let PropertyValue::Boolean(v) = value {
                    self.show_as_percent = *v;
                    return true;
                }
            }
            "label_position" => {
                if let PropertyValue::String(v) = value {
                    self.label_position = v.clone();
                    return true;
                }
            }
            "fill" => {
                if let PropertyValue::Boolean(v) = value {
                    self.fill = *v;
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    fn clone_box(&self) -> Box<dyn Primitive> {
        Box::new(self.clone())
    }
}

fn check_point_hit(sx: f64, sy: f64, px: f64, py: f64) -> bool {
    let radius = 8.0;
    (sx - px).powi(2) + (sy - py).powi(2) <= radius * radius
}

fn point_to_line_distance(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;

    if len_sq == 0.0 {
        return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt();
    }

    let t = ((px - x1) * dx + (py - y1) * dy) / len_sq;
    let t = t.clamp(0.0, 1.0);

    let proj_x = x1 + t * dx;
    let proj_y = y1 + t * dy;

    ((px - proj_x).powi(2) + (py - proj_y).powi(2)).sqrt()
}

// =============================================================================
// Factory Registration
// =============================================================================

fn create_fib_wedge(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let (bar1, price1) = points.first().copied().unwrap_or((0.0, 0.0));
    let (bar2, price2) = points.get(1).copied().unwrap_or((bar1 + 20.0, price1 + 10.0));
    let (bar3, price3) = points.get(2).copied().unwrap_or((bar1 + 20.0, price1 - 10.0));
    Box::new(FibWedge::new(bar1, price1, bar2, price2, bar3, price3, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "fib_wedge",
        display_name: "Fib Wedge",
        kind: PrimitiveKind::Fibonacci,
        click_behavior: ClickBehavior::ThreePoint,
        tooltip: "Wedge with Fibonacci levels",
        icon: "fib_wedge",
        default_color: "#F7B93E",
        factory: create_fib_wedge,
        supports_text: true,
        has_levels: true,
        has_points_config: false,
    }
}
