//! Fibonacci Channel primitive
//!
//! A channel with parallel lines at Fibonacci ratios from the baseline.
//! Uses three points: two define the baseline, third defines the channel width.

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

/// Fibonacci Channel - parallel lines at Fib ratios
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FibChannel {
    /// Common primitive data
    pub data: PrimitiveData,
    /// First baseline point bar
    pub bar1: f64,
    /// First baseline point price
    pub price1: f64,
    /// Second baseline point bar
    pub bar2: f64,
    /// Second baseline point price
    pub price2: f64,
    /// Third point bar (defines channel width)
    pub bar3: f64,
    /// Third point price
    pub price3: f64,
    /// Fibonacci level configurations (with individual colors/widths)
    #[serde(default = "default_level_configs", deserialize_with = "deserialize_level_configs")]
    pub level_configs: Vec<FibLevelConfig>,
    /// Show price labels
    #[serde(default = "default_true")]
    pub show_prices: bool,
    /// Show percentage/level labels
    #[serde(default = "default_true")]
    pub show_percentages: bool,
    /// Extend lines
    #[serde(default = "default_true")]
    pub extend: bool,
    /// Label position: "left", "right", "center"
    #[serde(default = "default_label_position")]
    pub label_position: String,
    /// Show levels as percentages (true) or coefficients (false)
    #[serde(default = "default_true")]
    pub show_as_percent: bool,
}

fn default_true() -> bool { true }
fn default_label_position() -> String { "left".to_string() }

impl FibChannel {
    /// Create a new Fibonacci channel
    pub fn new(bar1: f64, price1: f64, bar2: f64, price2: f64, bar3: f64, price3: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData {
                type_id: "fib_channel".to_string(),
                display_name: "Fib Channel".to_string(),
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
            show_prices: true,
            show_percentages: true,
            extend: true,
            label_position: "left".to_string(),
            show_as_percent: true,
        }
    }

    /// Calculate the perpendicular offset for channel width
    fn channel_offset(&self) -> (f64, f64) {
        // Vector from point 1 to point 2
        let dx = self.bar2 - self.bar1;
        let dy = self.price2 - self.price1;

        // Point 3 offset from baseline
        // Project point 3 onto the baseline to get perpendicular distance
        let len_sq = dx * dx + dy * dy;
        if len_sq == 0.0 {
            return (0.0, self.price3 - self.price1);
        }

        let t = ((self.bar3 - self.bar1) * dx + (self.price3 - self.price1) * dy) / len_sq;
        let proj_bar = self.bar1 + t * dx;
        let proj_price = self.price1 + t * dy;

        (self.bar3 - proj_bar, self.price3 - proj_price)
    }
}

impl Primitive for FibChannel {
    fn type_id(&self) -> &'static str {
        "fib_channel"
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

        let (offset_bar, offset_price) = self.channel_offset();

        // Check each channel line (only visible ones)
        for cfg in &self.level_configs {
            if !cfg.visible {
                continue;
            }

            let lx1 = viewport.bar_to_x_f64(self.bar1 + offset_bar * cfg.level);
            let ly1 = viewport.price_to_y(self.price1 + offset_price * cfg.level, price_scale.price_min, price_scale.price_max);
            let lx2 = viewport.bar_to_x_f64(self.bar2 + offset_bar * cfg.level);
            let ly2 = viewport.price_to_y(self.price2 + offset_price * cfg.level, price_scale.price_min, price_scale.price_max);

            if point_to_line_distance_extended(screen_x, screen_y, lx1, ly1, lx2, ly2, self.extend) < HIT_TOLERANCE {
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
        let chart_width = ctx.chart_width();

        // Calculate channel offset (perpendicular from baseline to point 3)
        let (offset_bar, offset_price) = self.channel_offset();

        // Check if we need line gap on median (0.5 level) for v_align == Center
        let needs_gap = self.data.text.as_ref()
            .map(|t| !t.content.is_empty() && matches!(t.v_align, TextAlign::Center))
            .unwrap_or(false);

        // === FILL RENDERING (before lines so lines are on top) ===
        // Collect visible levels sorted by level value for fill rendering
        let mut visible_levels: Vec<(usize, f64, f64, f64, f64, f64)> = self.level_configs
            .iter()
            .enumerate()
            .filter(|(_, cfg)| cfg.visible)
            .map(|(idx, cfg)| {
                let level = cfg.level;
                let lx1 = ctx.bar_to_x(self.bar1 + offset_bar * level);
                let ly1 = ctx.price_to_y(self.price1 + offset_price * level);
                let lx2 = ctx.bar_to_x(self.bar2 + offset_bar * level);
                let ly2 = ctx.price_to_y(self.price2 + offset_price * level);
                (idx, level, lx1, ly1, lx2, ly2)
            })
            .collect();
        visible_levels.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        // Draw fills between adjacent visible channel lines
        for i in 0..visible_levels.len().saturating_sub(1) {
            let (idx, _, ax1, ay1, ax2, ay2) = visible_levels[i];
            let (_, _, bx1, by1, bx2, by2) = visible_levels[i + 1];

            let cfg = &self.level_configs[idx];
            if cfg.fill_enabled {
                let fill_color = cfg.fill_color.as_deref()
                    .or(cfg.color.as_deref())
                    .unwrap_or(&self.data.color.stroke);

                // Calculate extended endpoints if extend is enabled
                let (ex_ax1, ex_ay1, ex_ax2, ex_ay2, ex_bx1, ex_by1, ex_bx2, ex_by2) = if self.extend {
                    let dx_a = ax2 - ax1;
                    let dy_a = ay2 - ay1;
                    let len_a = (dx_a * dx_a + dy_a * dy_a).sqrt();
                    let dx_b = bx2 - bx1;
                    let dy_b = by2 - by1;
                    let len_b = (dx_b * dx_b + dy_b * dy_b).sqrt();
                    let ext = chart_width * 2.0;

                    let (eax1, eay1, eax2, eay2) = if len_a > 0.0 {
                        let nx = dx_a / len_a;
                        let ny = dy_a / len_a;
                        (ax1 - nx * ext, ay1 - ny * ext, ax1 + nx * ext, ay1 + ny * ext)
                    } else {
                        (ax1, ay1, ax2, ay2)
                    };
                    let (ebx1, eby1, ebx2, eby2) = if len_b > 0.0 {
                        let nx = dx_b / len_b;
                        let ny = dy_b / len_b;
                        (bx1 - nx * ext, by1 - ny * ext, bx1 + nx * ext, by1 + ny * ext)
                    } else {
                        (bx1, by1, bx2, by2)
                    };
                    (eax1, eay1, eax2, eay2, ebx1, eby1, ebx2, eby2)
                } else {
                    (ax1, ay1, ax2, ay2, bx1, by1, bx2, by2)
                };

                // Draw fill quad between two channel lines
                ctx.set_fill_color_alpha(fill_color, cfg.fill_opacity);
                ctx.begin_path();
                ctx.move_to(ex_ax1, ex_ay1);
                ctx.line_to(ex_ax2, ex_ay2);
                ctx.line_to(ex_bx2, ex_by2);
                ctx.line_to(ex_bx1, ex_by1);
                ctx.close_path();
                ctx.fill();
                ctx.reset_alpha();
            }
        }

        // Draw each channel level line (only visible ones)
        for cfg in &self.level_configs {
            if !cfg.visible {
                continue;
            }

            let level = cfg.level;
            let lx1 = ctx.bar_to_x(self.bar1 + offset_bar * level);
            let ly1 = ctx.price_to_y(self.price1 + offset_price * level);
            let lx2 = ctx.bar_to_x(self.bar2 + offset_bar * level);
            let ly2 = ctx.price_to_y(self.price2 + offset_price * level);

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
                _ => LineStyle::Solid,
            };

            match line_style {
                LineStyle::Solid => ctx.set_line_dash(&[]),
                LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
                LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
                LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
                LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
            }

            // Check if this is the median line (0.5 level) and needs gap
            let is_median = (level - 0.5).abs() < 0.001;

            // Build label text if needed
            let label = if self.show_prices || self.show_percentages {
                let mut label_parts = Vec::new();
                if self.show_percentages {
                    if self.show_as_percent {
                        let pct = level * 100.0;
                        if (pct - pct.round()).abs() < 0.01 {
                            label_parts.push(format!("{}%", pct as i32));
                        } else {
                            label_parts.push(format!("{:.1}%", pct));
                        }
                    } else {
                        let lvl = level;
                        if (lvl - lvl.round()).abs() < 0.0001 {
                            label_parts.push(format!("{}", lvl as i32));
                        } else if (lvl * 10.0 - (lvl * 10.0).round()).abs() < 0.001 {
                            label_parts.push(format!("{:.1}", lvl));
                        } else {
                            label_parts.push(format!("{:.3}", lvl));
                        }
                    }
                }
                if self.show_prices {
                    // Price at this level point (use average of line endpoints)
                    let level_price = (self.price1 + offset_price * level + self.price2 + offset_price * level) / 2.0;
                    label_parts.push(super::super::fmt_price(level_price));
                }
                Some(label_parts.join(" "))
            } else {
                None
            };

            // Calculate label gap position based on label_position
            let label_gap = if let Some(ref lbl) = label {
                let char_width = 6.5;
                let text_width = lbl.len() as f64 * char_width;
                let dx = lx2 - lx1;
                let dy = ly2 - ly1;
                let line_len = (dx * dx + dy * dy).sqrt();
                if line_len > 0.001 {
                    let half_gap_t = (text_width / 2.0) / line_len;
                    match self.label_position.as_str() {
                        "right" => Some((1.0 - half_gap_t, 1.0)),
                        "center" => Some((0.5 - half_gap_t, 0.5 + half_gap_t)),
                        _ => Some((0.0, half_gap_t * 2.0)), // "left"
                    }
                } else {
                    None
                }
            } else {
                None
            };

            ctx.begin_path();
            if is_median && needs_gap {
                // Draw median line with gap
                let text = self.data.text.as_ref().unwrap();
                let dx = lx2 - lx1;
                let dy = ly2 - ly1;
                let base_len = (dx * dx + dy * dy).sqrt();

                if base_len > 0.001 {
                    let t_center = match text.h_align {
                        TextAlign::Start => 0.0,
                        TextAlign::Center => 0.5,
                        TextAlign::End => 1.0,
                    };

                    let char_count = text.content.len() as f64;
                    let text_width = char_count * text.font_size * 0.6 + 8.0;
                    let half_gap_t = (text_width / 2.0) / base_len;

                    let gap_t_start = (t_center - half_gap_t).max(0.0);
                    let gap_t_end = (t_center + half_gap_t).min(1.0);

                    // First segment [0, gap_t_start] or extended left
                    if gap_t_start > 0.001 {
                        let gap_x1 = lx1 + dx * gap_t_start;
                        let gap_y1 = ly1 + dy * gap_t_start;
                        if self.extend {
                            let ext = chart_width * 2.0;
                            let nx = dx / base_len;
                            let ny = dy / base_len;
                            ctx.move_to(crisp(lx1 - nx * ext, dpr), crisp(ly1 - ny * ext, dpr));
                        } else {
                            ctx.move_to(crisp(lx1, dpr), crisp(ly1, dpr));
                        }
                        ctx.line_to(crisp(gap_x1, dpr), crisp(gap_y1, dpr));
                    }
                    ctx.stroke();

                    // Second segment [gap_t_end, 1] or extended right
                    ctx.begin_path();
                    if gap_t_end < 0.999 {
                        let gap_x2 = lx1 + dx * gap_t_end;
                        let gap_y2 = ly1 + dy * gap_t_end;
                        ctx.move_to(crisp(gap_x2, dpr), crisp(gap_y2, dpr));
                        if self.extend {
                            let ext = chart_width * 2.0;
                            let nx = dx / base_len;
                            let ny = dy / base_len;
                            ctx.line_to(crisp(lx2 + nx * ext, dpr), crisp(ly2 + ny * ext, dpr));
                        } else {
                            ctx.line_to(crisp(lx2, dpr), crisp(ly2, dpr));
                        }
                    }
                    ctx.stroke();
                }
            } else if let Some((gap_t_start, gap_t_end)) = label_gap {
                // Draw line with gap for label
                let dx = lx2 - lx1;
                let dy = ly2 - ly1;
                let base_len = (dx * dx + dy * dy).sqrt();

                if base_len > 0.001 {
                    // First segment
                    if gap_t_start > 0.001 {
                        let gap_x1 = lx1 + dx * gap_t_start;
                        let gap_y1 = ly1 + dy * gap_t_start;
                        if self.extend {
                            let ext = chart_width * 2.0;
                            let nx = dx / base_len;
                            let ny = dy / base_len;
                            ctx.move_to(crisp(lx1 - nx * ext, dpr), crisp(ly1 - ny * ext, dpr));
                        } else {
                            ctx.move_to(crisp(lx1, dpr), crisp(ly1, dpr));
                        }
                        ctx.line_to(crisp(gap_x1, dpr), crisp(gap_y1, dpr));
                    }
                    ctx.stroke();

                    // Second segment
                    ctx.begin_path();
                    if gap_t_end < 0.999 {
                        let gap_x2 = lx1 + dx * gap_t_end;
                        let gap_y2 = ly1 + dy * gap_t_end;
                        ctx.move_to(crisp(gap_x2, dpr), crisp(gap_y2, dpr));
                        if self.extend {
                            let ext = chart_width * 2.0;
                            let nx = dx / base_len;
                            let ny = dy / base_len;
                            ctx.line_to(crisp(lx2 + nx * ext, dpr), crisp(ly2 + ny * ext, dpr));
                        } else {
                            ctx.line_to(crisp(lx2, dpr), crisp(ly2, dpr));
                        }
                    }
                    ctx.stroke();
                }
            } else if self.extend {
                // Extend the line in both directions
                let dx = lx2 - lx1;
                let dy = ly2 - ly1;
                let len = (dx * dx + dy * dy).sqrt();
                if len > 0.0 {
                    let ext = chart_width * 2.0;
                    let nx = dx / len;
                    let ny = dy / len;
                    ctx.move_to(crisp(lx1 - nx * ext, dpr), crisp(ly1 - ny * ext, dpr));
                    ctx.line_to(crisp(lx2 + nx * ext, dpr), crisp(ly2 + ny * ext, dpr));
                }
                ctx.stroke();
            } else {
                ctx.move_to(crisp(lx1, dpr), crisp(ly1, dpr));
                ctx.line_to(crisp(lx2, dpr), crisp(ly2, dpr));
                ctx.stroke();
            }

            // Draw label
            if let Some(ref lbl) = label {
                ctx.set_font("11px sans-serif");
                ctx.set_text_baseline(crate::render::TextBaseline::Middle);
                ctx.set_fill_color(color);

                let dx = lx2 - lx1;
                let dy = ly2 - ly1;

                match self.label_position.as_str() {
                    "right" => {
                        ctx.set_text_align(crate::render::TextAlign::Right);
                        ctx.fill_text(lbl, lx2, ly2);
                    }
                    "center" => {
                        ctx.set_text_align(crate::render::TextAlign::Center);
                        ctx.fill_text(lbl, lx1 + dx / 2.0, ly1 + dy / 2.0);
                    }
                    _ => { // "left"
                        ctx.set_text_align(crate::render::TextAlign::Left);
                        ctx.fill_text(lbl, lx1, ly1);
                    }
                }
            }
        }
        ctx.set_line_dash(&[]);

        // Render text if present (rotated along the appropriate line based on v_align)
        if let Some(ref text) = self.data.text {
            if !text.content.is_empty() {
                // Calculate text position based on v_align:
                // - Start: above upper boundary (level 1.0)
                // - Center: on median line (level 0.5) - with line gap already drawn
                // - End: below lower boundary (level 0.0)
                let (text_start_x, text_start_y, text_end_x, text_end_y) = match text.v_align {
                    TextAlign::Start | TextAlign::End => {
                        // Find upper (1.0) and lower (0.0) boundary lines
                        // Upper boundary in screen coords (smaller Y) vs lower (larger Y)
                        let level_0_y1 = ctx.price_to_y(self.price1 + offset_price * 0.0);
                        let level_1_y1 = ctx.price_to_y(self.price1 + offset_price * 1.0);

                        // In screen coords, smaller Y is visually higher
                        let (upper_level, lower_level) = if level_1_y1 < level_0_y1 {
                            (1.0, 0.0)
                        } else {
                            (0.0, 1.0)
                        };

                        let level = if matches!(text.v_align, TextAlign::Start) {
                            upper_level  // Start = upper boundary
                        } else {
                            lower_level  // End = lower boundary
                        };

                        let lx1 = ctx.bar_to_x(self.bar1 + offset_bar * level);
                        let ly1 = ctx.price_to_y(self.price1 + offset_price * level);
                        let lx2 = ctx.bar_to_x(self.bar2 + offset_bar * level);
                        let ly2 = ctx.price_to_y(self.price2 + offset_price * level);
                        (lx1, ly1, lx2, ly2)
                    }
                    TextAlign::Center => {
                        // On median line (level 0.5)
                        let lx1 = ctx.bar_to_x(self.bar1 + offset_bar * 0.5);
                        let ly1 = ctx.price_to_y(self.price1 + offset_price * 0.5);
                        let lx2 = ctx.bar_to_x(self.bar2 + offset_bar * 0.5);
                        let ly2 = ctx.price_to_y(self.price2 + offset_price * 0.5);
                        (lx1, ly1, lx2, ly2)
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
            ConfigProperty::show_prices(self.show_prices).with_order(10),
            ConfigProperty::levels(self.show_percentages).with_order(11),
            ConfigProperty::show_as_percent(self.show_as_percent).with_order(12),
            ConfigProperty::label_position(&self.label_position).with_order(13),
            ConfigProperty::extend_lines(self.extend).with_order(20),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &super::super::config::PropertyValue) -> bool {
        use super::super::config::PropertyValue;
        match id {
            "show_prices" => {
                if let PropertyValue::Boolean(v) = value {
                    self.show_prices = *v;
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
            "extend" => {
                if let PropertyValue::Boolean(v) = value {
                    self.extend = *v;
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

fn point_to_line_distance_extended(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64, extend: bool) -> f64 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;

    if len_sq == 0.0 {
        return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt();
    }

    let t = ((px - x1) * dx + (py - y1) * dy) / len_sq;
    let t = if extend { t } else { t.clamp(0.0, 1.0) };

    let proj_x = x1 + t * dx;
    let proj_y = y1 + t * dy;

    ((px - proj_x).powi(2) + (py - proj_y).powi(2)).sqrt()
}

// =============================================================================
// Factory Registration
// =============================================================================

fn create_fib_channel(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive> {
    let (bar1, price1) = points.first().copied().unwrap_or((0.0, 0.0));
    let (bar2, price2) = points.get(1).copied().unwrap_or((bar1 + 20.0, price1 + 10.0));
    let (bar3, price3) = points.get(2).copied().unwrap_or((bar1, price1 + 20.0));
    Box::new(FibChannel::new(bar1, price1, bar2, price2, bar3, price3, color))
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "fib_channel",
        display_name: "Fib Channel",
        kind: PrimitiveKind::Fibonacci,
        click_behavior: ClickBehavior::ThreePoint,
        tooltip: "Fibonacci channel with parallel levels",
        icon: "fib_channel",
        default_color: "#F7B93E",
        factory: create_fib_channel,
        supports_text: true,
        has_levels: true,
        has_points_config: false,
    }
}
