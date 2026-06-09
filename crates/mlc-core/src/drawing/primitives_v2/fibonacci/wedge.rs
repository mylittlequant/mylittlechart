//! Fibonacci Wedge primitive
//!
//! A wedge/triangle shape with Fibonacci levels inside.
//! Three points define the wedge, levels are drawn between the sides.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64};
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
    pub data: PrimitiveData,
    #[serde(default)]
    pub ts1: i64,
    pub price1: f64,
    #[serde(default)]
    pub ts2: i64,
    pub price2: f64,
    #[serde(default)]
    pub ts3: i64,
    pub price3: f64,
    #[serde(default = "default_level_configs", deserialize_with = "deserialize_level_configs")]
    pub level_configs: Vec<FibLevelConfig>,
    #[serde(default = "default_true")]
    pub show_labels: bool,
    #[serde(default = "default_true")]
    pub show_percentages: bool,
    #[serde(default = "default_label_position")]
    pub label_position: String,
    #[serde(default = "default_true")]
    pub show_as_percent: bool,
    #[serde(default)]
    pub fill: bool,
    #[serde(default = "default_fill_opacity")]
    pub fill_opacity: f64,
}

fn default_true() -> bool { true }
fn default_fill_opacity() -> f64 { 0.1 }
fn default_label_position() -> String { "left".to_string() }

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
        LevelFormat::Levels(levels) => Ok(levels.iter().map(|&level| FibLevelConfig::new(level)).collect()),
    }
}

impl FibWedge {
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, ts3: i64, price3: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "fib_wedge".to_string(),
                display_name: "Fib Wedge".to_string(),
                color: PrimitiveColor::new(color),
                width: 1.0,
                ..Default::default()
            },
            ts1, price1, ts2, price2, ts3, price3,
            level_configs: default_level_configs(),
            show_labels: true,
            show_percentages: true,
            label_position: "left".to_string(),
            show_as_percent: true,
            fill: false,
            fill_opacity: 0.1,
        }
    }

    /// Upper edge screen point at parameter t (0=apex/p1, 1=p2 corner)
    fn upper_edge_screen(t: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> (f64, f64) {
        (x1 + t * (x2 - x1), y1 + t * (y2 - y1))
    }

    /// Lower edge screen point at parameter t (0=apex/p1, 1=p3 corner)
    fn lower_edge_screen(t: f64, x1: f64, y1: f64, x3: f64, y3: f64) -> (f64, f64) {
        (x1 + t * (x3 - x1), y1 + t * (y3 - y1))
    }
}

impl Primitive for FibWedge {
    fn type_id(&self) -> &'static str { "fib_wedge" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Fibonacci }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::ThreePoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }

    fn points(&self) -> Vec<(i64, f64)> {
        vec![(self.ts1, self.price1), (self.ts2, self.price2), (self.ts3, self.price3)]
    }

    fn set_points(&mut self, points: &[(i64, f64)]) {
        if let Some(&(ts, price)) = points.first() { self.ts1 = ts; self.price1 = price; }
        if let Some(&(ts, price)) = points.get(1) { self.ts2 = ts; self.price2 = price; }
        if let Some(&(ts, price)) = points.get(2) { self.ts3 = ts; self.price3 = price; }
    }

    fn translate(&mut self, td: i64, pd: f64) {
        self.ts1 += td; self.ts2 += td; self.ts3 += td;
        self.price1 += pd; self.price2 += pd; self.price3 += pd;
    }

    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Point1 => { self.ts1 = ts_ms; self.price1 = price; }
            ControlPointType::Point2 => { self.ts2 = ts_ms; self.price2 = price; }
            ControlPointType::Point3 => { self.ts3 = ts_ms; self.price3 = price; }
            ControlPointType::Move => {
                let td = ts_ms - self.ts1; let pd = price - self.price1;
                self.translate(td, pd);
            }
            _ => {}
        }
    }

    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let b1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let b2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let b3 = timestamp_ms_to_bar_f64(bars, self.ts3);
        let x1 = vp.bar_to_x_f64(b1);
        let y1 = vp.price_to_y(self.price1, ps.price_min, ps.price_max);
        let x2 = vp.bar_to_x_f64(b2);
        let y2 = vp.price_to_y(self.price2, ps.price_min, ps.price_max);
        let x3 = vp.bar_to_x_f64(b3);
        let y3 = vp.price_to_y(self.price3, ps.price_min, ps.price_max);

        if check_point_hit(sx, sy, x1, y1) { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if check_point_hit(sx, sy, x2, y2) { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        if check_point_hit(sx, sy, x3, y3) { return HitTestResult::ControlPoint(ControlPointType::Point3); }

        if point_to_line_distance(sx, sy, x1, y1, x2, y2) < HIT_TOLERANCE { return HitTestResult::Body; }
        if point_to_line_distance(sx, sy, x1, y1, x3, y3) < HIT_TOLERANCE { return HitTestResult::Body; }
        if point_to_line_distance(sx, sy, x2, y2, x3, y3) < HIT_TOLERANCE { return HitTestResult::Body; }

        for cfg in &self.level_configs {
            if !cfg.visible { continue; }
            let (ux, uy) = Self::upper_edge_screen(cfg.level, x1, y1, x2, y2);
            let (lx, ly) = Self::lower_edge_screen(cfg.level, x1, y1, x3, y3);
            if point_to_line_distance(sx, sy, ux, uy, lx, ly) < HIT_TOLERANCE {
                return HitTestResult::Body;
            }
        }
        HitTestResult::Miss
    }

    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let b1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let b2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let b3 = timestamp_ms_to_bar_f64(bars, self.ts3);
        let x1 = vp.bar_to_x_f64(b1);
        let y1 = vp.price_to_y(self.price1, ps.price_min, ps.price_max);
        let x2 = vp.bar_to_x_f64(b2);
        let y2 = vp.price_to_y(self.price2, ps.price_min, ps.price_max);
        let x3 = vp.bar_to_x_f64(b3);
        let y3 = vp.price_to_y(self.price3, ps.price_min, ps.price_max);
        vec![ControlPoint::point1(x1, y1), ControlPoint::point2(x2, y2), ControlPoint::point3(x3, y3)]
    }

    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let y2 = ctx.price_to_y(self.price2);
        let x3 = ctx.ts_to_x_ms(self.ts3);
        let y3 = ctx.price_to_y(self.price3);

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

        ctx.begin_path();
        ctx.move_to(crisp(x1, dpr), crisp(y1, dpr));
        ctx.line_to(crisp(x2, dpr), crisp(y2, dpr));
        ctx.line_to(crisp(x3, dpr), crisp(y3, dpr));
        ctx.close_path();
        ctx.stroke();

        let needs_gap = self.data.text.as_ref()
            .map(|t| !t.content.is_empty() && matches!(t.v_align, TextAlign::Center))
            .unwrap_or(false);

        // === FILL levels ===
        let mut visible_levels: Vec<(usize, f64, f64, f64, f64, f64)> = self.level_configs
            .iter()
            .enumerate()
            .filter(|(_, cfg)| cfg.visible)
            .map(|(idx, cfg)| {
                let (ux, uy) = Self::upper_edge_screen(cfg.level, x1, y1, x2, y2);
                let (lx, ly) = Self::lower_edge_screen(cfg.level, x1, y1, x3, y3);
                (idx, cfg.level, ux, uy, lx, ly)
            })
            .collect();
        visible_levels.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        for i in 0..visible_levels.len().saturating_sub(1) {
            let (idx, _, ux1, uy1, lx1, ly1) = visible_levels[i];
            let (_, _, ux2, uy2, lx2, ly2) = visible_levels[i + 1];
            let cfg = &self.level_configs[idx];
            if cfg.fill_enabled {
                let fill_color = cfg.fill_color.as_deref()
                    .or(cfg.color.as_deref())
                    .unwrap_or(&self.data.color.stroke);
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

        for cfg in &self.level_configs {
            if !cfg.visible { continue; }

            let (ux, uy) = Self::upper_edge_screen(cfg.level, x1, y1, x2, y2);
            let (lx, ly) = Self::lower_edge_screen(cfg.level, x1, y1, x3, y3);

            let color = cfg.color.as_deref().unwrap_or(&self.data.color.stroke);
            ctx.set_stroke_color(color);
            let width = cfg.width.unwrap_or(self.data.width);
            ctx.set_stroke_width(width);

            let line_style = match cfg.style.as_str() {
                "dashed" => LineStyle::Dashed,
                "dotted" => LineStyle::Dotted,
                "large_dashed" => LineStyle::LargeDashed,
                "sparse_dotted" => LineStyle::SparseDotted,
                _ => self.data.style,
            };
            match line_style {
                LineStyle::Solid => ctx.set_line_dash(&[]),
                LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
                LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
                LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
                LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
            }

            let dx = lx - ux; let dy = ly - uy;
            let line_len = (dx * dx + dy * dy).sqrt();

            let (label, label_gap_t_start, label_gap_t_end) = if self.show_labels || self.show_percentages {
                let label = {
                    let mut label_parts = Vec::new();
                    if self.show_percentages {
                        if self.show_as_percent {
                            let pct = cfg.level * 100.0;
                            if (pct - pct.round()).abs() < 0.01 { label_parts.push(format!("{}%", pct as i32)); }
                            else { label_parts.push(format!("{:.1}%", pct)); }
                        } else {
                            let lvl = cfg.level;
                            if (lvl - lvl.round()).abs() < 0.0001 { label_parts.push(format!("{}", lvl as i32)); }
                            else if (lvl * 10.0 - (lvl * 10.0).round()).abs() < 0.001 { label_parts.push(format!("{:.1}", lvl)); }
                            else { label_parts.push(format!("{:.3}", lvl)); }
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
                        _ => (0.0, (half_gap_t * 2.0).min(1.0)),
                    };
                    (Some(label), gap_start, gap_end)
                } else {
                    (if label.is_empty() { None } else { Some(label) }, 0.0, 0.0)
                }
            } else { (None, 0.0, 0.0) };

            let is_median = (cfg.level - 0.5).abs() < 0.001;
            let has_label_gap = label.is_some() && label_gap_t_end > label_gap_t_start;

            let (use_gap, gap_t_start, gap_t_end) = if has_label_gap {
                (true, label_gap_t_start, label_gap_t_end)
            } else if is_median && needs_gap && line_len > 0.001 {
                let text = self.data.text.as_ref().unwrap();
                let t_center = match text.h_align {
                    TextAlign::Start => 0.0, TextAlign::Center => 0.5, TextAlign::End => 1.0,
                };
                let char_count = text.content.len() as f64;
                let text_width = char_count * text.font_size * 0.6 + 8.0;
                let half_gap_t = (text_width / 2.0) / line_len;
                (true, (t_center - half_gap_t).max(0.0), (t_center + half_gap_t).min(1.0))
            } else { (false, 0.0, 0.0) };

            ctx.begin_path();
            if use_gap && gap_t_end > gap_t_start {
                if gap_t_start > 0.001 {
                    let gap_x1 = ux + dx * gap_t_start; let gap_y1 = uy + dy * gap_t_start;
                    ctx.move_to(crisp(ux, dpr), crisp(uy, dpr));
                    ctx.line_to(crisp(gap_x1, dpr), crisp(gap_y1, dpr));
                }
                if gap_t_end < 0.999 {
                    let gap_x2 = ux + dx * gap_t_end; let gap_y2 = uy + dy * gap_t_end;
                    ctx.move_to(crisp(gap_x2, dpr), crisp(gap_y2, dpr));
                    ctx.line_to(crisp(lx, dpr), crisp(ly, dpr));
                }
            } else {
                ctx.move_to(crisp(ux, dpr), crisp(uy, dpr));
                ctx.line_to(crisp(lx, dpr), crisp(ly, dpr));
            }
            ctx.stroke();

            if let Some(lbl) = label {
                ctx.set_font("11px sans-serif");
                ctx.set_text_baseline(crate::render::TextBaseline::Middle);
                ctx.set_fill_color(color);
                match self.label_position.as_str() {
                    "right" => { ctx.set_text_align(crate::render::TextAlign::Right); ctx.fill_text(&lbl, lx, ly); }
                    "center" => { ctx.set_text_align(crate::render::TextAlign::Center); ctx.fill_text(&lbl, (ux + lx) / 2.0, (uy + ly) / 2.0); }
                    _ => { ctx.set_text_align(crate::render::TextAlign::Left); ctx.fill_text(&lbl, ux, uy); }
                }
            }
        }
        ctx.set_line_dash(&[]);

        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                let (text_start_x, text_start_y, text_end_x, text_end_y) = match text.v_align {
                    TextAlign::Start => {
                        let (ux, uy) = Self::upper_edge_screen(1.0, x1, y1, x2, y2);
                        (x1, y1, ux, uy)
                    }
                    TextAlign::Center => {
                        let (ux, uy) = Self::upper_edge_screen(0.5, x1, y1, x2, y2);
                        let (lx, ly) = Self::lower_edge_screen(0.5, x1, y1, x3, y3);
                        (ux, uy, lx, ly)
                    }
                    TextAlign::End => {
                        let (lx, ly) = Self::lower_edge_screen(1.0, x1, y1, x3, y3);
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
                ctx.arc(px, py, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.stroke();
            }
        }
    }

    fn level_configs(&self) -> Option<Vec<FibLevelConfig>> { Some(self.level_configs.clone()) }
    fn set_level_configs(&mut self, configs: Vec<FibLevelConfig>) -> bool { self.level_configs = configs; true }

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
            "show_labels" => { if let PropertyValue::Boolean(v) = value { self.show_labels = *v; return true; } }
            "show_percentages" => { if let PropertyValue::Boolean(v) = value { self.show_percentages = *v; return true; } }
            "show_as_percent" => { if let PropertyValue::Boolean(v) = value { self.show_as_percent = *v; return true; } }
            "label_position" => { if let PropertyValue::String(v) = value { self.label_position = v.clone(); return true; } }
            "fill" => { if let PropertyValue::Boolean(v) = value { self.fill = *v; return true; } }
            _ => {}
        }
        false
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }
}

fn check_point_hit(sx: f64, sy: f64, px: f64, py: f64) -> bool {
    let radius = 8.0;
    (sx - px).powi(2) + (sy - py).powi(2) <= radius * radius
}

fn point_to_line_distance(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let dx = x2 - x1; let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;
    if len_sq == 0.0 { return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt(); }
    let t = (((px - x1) * dx + (py - y1) * dy) / len_sq).clamp(0.0, 1.0);
    let proj_x = x1 + t * dx; let proj_y = y1 + t * dy;
    ((px - proj_x).powi(2) + (py - proj_y).powi(2)).sqrt()
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
        factory: |points, color| {
            let (ts1, price1) = points.first().copied().unwrap_or((0, 0.0));
            let (ts2, price2) = points.get(1).copied().unwrap_or((ts1 + 1_200_000, price1 + 10.0));
            let (ts3, price3) = points.get(2).copied().unwrap_or((ts1 + 1_200_000, price1 - 10.0));
            Box::new(FibWedge::new(ts1, price1, ts2, price2, ts3, price3, color))
        },
        supports_text: true,
        has_levels: true,
        has_points_config: false,
    }
}
