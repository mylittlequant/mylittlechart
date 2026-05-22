//! Table primitive - data table annotation

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport, timestamp_ms_to_bar_f64, i18n::{ConfigKey, current_language}};
use super::super::{
    Primitive, PrimitiveData, PrimitiveKind, ClickBehavior, HitTestResult,
    PrimitiveMetadata, ControlPoint, ControlPointType, PrimitiveColor,
    RenderContext, crisp, CONTROL_POINT_RADIUS, CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
};
use super::super::config::{ConfigProperty, PropertyValue, PropertyCategory};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Table {
    pub data: PrimitiveData,
    pub ts1: i64, pub price1: f64, // Top-left corner
    pub ts2: i64, pub price2: f64, // Bottom-right corner
    pub rows: Vec<Vec<String>>,
    #[serde(default = "default_cols")] pub columns: u8,
    #[serde(default = "default_true")] pub show_header: bool,
    #[serde(default = "default_header_color")] pub header_color: String,
    #[serde(default = "default_grid_color")] pub grid_color: String,
    #[serde(default = "default_text_color")] pub text_color: String,
    #[serde(default = "default_header_text_color")] pub header_text_color: String,
}
fn default_cols() -> u8 { 2 }
fn default_true() -> bool { true }
fn default_header_color() -> String { "#607D8B".to_string() }
fn default_grid_color() -> String { "#607D8B".to_string() }
fn default_text_color() -> String { "#FFFFFF".to_string() }
fn default_header_text_color() -> String { "#000000".to_string() }

impl Table {
    pub fn new(ts1: i64, price1: f64, ts2: i64, price2: f64, color: &str) -> Self {
        Self {
            data: PrimitiveData { type_id: "table".to_string(), display_name: "Table".to_string(), color: PrimitiveColor::new(color), width: 1.0, ..Default::default() },
            ts1, price1, ts2, price2,
            rows: vec![vec!["Header1".to_string(), "Header2".to_string()], vec!["Value1".to_string(), "Value2".to_string()]],
            columns: 2,
            show_header: true,
            header_color: color.to_string(),
            grid_color: color.to_string(),
            text_color: "#FFFFFF".to_string(),
            header_text_color: "#000000".to_string(),
        }
    }
}

impl Primitive for Table {
    fn type_id(&self) -> &'static str { "table" }
    fn display_name(&self) -> &str { &self.data.display_name }
    fn kind(&self) -> PrimitiveKind { PrimitiveKind::Annotation }
    fn click_behavior(&self) -> ClickBehavior { ClickBehavior::TwoPoint }
    fn data(&self) -> &PrimitiveData { &self.data }
    fn data_mut(&mut self) -> &mut PrimitiveData { &mut self.data }
    fn points(&self) -> Vec<(i64, f64)> { vec![(self.ts1, self.price1), (self.ts2, self.price2)] }
    fn set_points(&mut self, points: &[(i64, f64)]) {
        if let Some(&(t, p)) = points.first() { self.ts1 = t; self.price1 = p; }
        if let Some(&(t, p)) = points.get(1) { self.ts2 = t; self.price2 = p; }
    }
    fn translate(&mut self, ts_delta_ms: i64, pd: f64) { self.ts1 += ts_delta_ms; self.ts2 += ts_delta_ms; self.price1 += pd; self.price2 += pd; }
    fn move_control_point(&mut self, pt: ControlPointType, ts_ms: i64, price: f64) {
        match pt {
            ControlPointType::Point1 => { self.ts1 = ts_ms; self.price1 = price; }
            ControlPointType::Point2 => { self.ts2 = ts_ms; self.price2 = price; }
            ControlPointType::Move => { let td = ts_ms - self.ts1; let pd = price - self.price1; self.translate(td, pd); }
            _ => {}
        }
    }
    fn hit_test(&self, sx: f64, sy: f64, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> HitTestResult {
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        let (x1, y1) = (vp.bar_to_x_f64(bar1), vp.price_to_y(self.price1, ps.price_min, ps.price_max));
        let (x2, y2) = (vp.bar_to_x_f64(bar2), vp.price_to_y(self.price2, ps.price_min, ps.price_max));
        let r = 8.0;
        if (sx - x1).powi(2) + (sy - y1).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point1); }
        if (sx - x2).powi(2) + (sy - y2).powi(2) <= r * r { return HitTestResult::ControlPoint(ControlPointType::Point2); }
        let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
        let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };
        if sx >= min_x && sx <= max_x && sy >= min_y && sy <= max_y {
            return HitTestResult::Body;
        }
        HitTestResult::Miss
    }
    fn control_points(&self, bars: &[Bar], vp: &Viewport, ps: &PriceScale) -> Vec<ControlPoint> {
        let bar1 = timestamp_ms_to_bar_f64(bars, self.ts1);
        let bar2 = timestamp_ms_to_bar_f64(bars, self.ts2);
        vec![
            ControlPoint::point1(vp.bar_to_x_f64(bar1), vp.price_to_y(self.price1, ps.price_min, ps.price_max)),
            ControlPoint::point2(vp.bar_to_x_f64(bar2), vp.price_to_y(self.price2, ps.price_min, ps.price_max)),
        ]
    }
    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        let dpr = ctx.dpr();
        let x1 = ctx.ts_to_x_ms(self.ts1);
        let y1 = ctx.price_to_y(self.price1);
        let x2 = ctx.ts_to_x_ms(self.ts2);
        let y2 = ctx.price_to_y(self.price2);

        let (left, right) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
        let (top, bottom) = if y1 < y2 { (y1, y2) } else { (y2, y1) };
        let total_w = (right - left).abs().max(1.0);
        let total_h = (bottom - top).abs().max(1.0);

        let cell_w = total_w / self.columns.max(1) as f64;
        let cell_h = if self.rows.is_empty() { total_h } else { total_h / self.rows.len() as f64 };

        let padding_x = 6.0;
        let font_size = 11.0;

        ctx.set_fill_color(&format!("{}E0", &self.data.color.stroke));
        ctx.fill_rect(crisp(left, dpr), crisp(top, dpr), total_w, total_h);

        if self.show_header && !self.rows.is_empty() {
            ctx.set_fill_color(&self.header_color);
            ctx.fill_rect(crisp(left, dpr), crisp(top, dpr), total_w, cell_h);
        }

        ctx.set_stroke_color(&self.grid_color);
        ctx.set_stroke_width(1.0);

        for i in 0..=self.rows.len() {
            let ly = top + i as f64 * cell_h;
            ctx.begin_path();
            ctx.move_to(crisp(left, dpr), crisp(ly, dpr));
            ctx.line_to(crisp(right, dpr), crisp(ly, dpr));
            ctx.stroke();
        }

        for i in 0..=self.columns {
            let lx = left + i as f64 * cell_w;
            ctx.begin_path();
            ctx.move_to(crisp(lx, dpr), crisp(top, dpr));
            ctx.line_to(crisp(lx, dpr), crisp(bottom, dpr));
            ctx.stroke();
        }

        ctx.set_font(&format!("{}px sans-serif", font_size as i32));
        ctx.set_text_align(crate::render::TextAlign::Left);
        for (row_idx, row) in self.rows.iter().enumerate() {
            let is_header = row_idx == 0 && self.show_header;
            ctx.set_fill_color(if is_header { &self.header_text_color } else { &self.text_color });

            for (col_idx, cell) in row.iter().enumerate() {
                if col_idx < self.columns as usize {
                    let cell_left = left + col_idx as f64 * cell_w;
                    let cell_top = top + row_idx as f64 * cell_h;
                    let text_x = cell_left + padding_x;
                    let text_y = cell_top + (cell_h + font_size) / 2.0 - 2.0;
                    ctx.fill_text(cell, text_x, text_y);
                }
            }
        }

        if is_selected {
            ctx.set_stroke_color(CONTROL_POINT_STROKE);
            ctx.set_fill_color(CONTROL_POINT_FILL);
            ctx.set_stroke_width(1.5);
            for (px, py) in [(x1, y1), (x2, y2)] {
                ctx.begin_path();
                ctx.arc(px, py, CONTROL_POINT_RADIUS, 0.0, std::f64::consts::TAU);
                ctx.fill();
                ctx.stroke();
            }
        }
    }

    fn to_json(&self) -> String { serde_json::to_string(self).unwrap_or_default() }
    fn clone_box(&self) -> Box<dyn Primitive> { Box::new(self.clone()) }

    fn text_properties(&self) -> Option<Vec<ConfigProperty>> {
        let mut props = Vec::new();
        let lang = current_language();

        props.push(
            ConfigProperty::rows_count(self.rows.len() as f64)
                .with_category(PropertyCategory::Text)
                .with_order(0)
        );
        props.push(
            ConfigProperty::columns_count(self.columns as f64)
                .with_category(PropertyCategory::Text)
                .with_order(1)
        );
        props.push(
            ConfigProperty::show_header(self.show_header)
                .with_category(PropertyCategory::Text)
                .with_order(2)
        );

        let header_text = ConfigKey::Header.get(lang);
        let cell_text = ConfigKey::Cell.get(lang);
        for (row_idx, row) in self.rows.iter().enumerate() {
            for (col_idx, cell_value) in row.iter().enumerate() {
                if col_idx < self.columns as usize {
                    let id = format!("cell_{}_{}", row_idx, col_idx);
                    let label = if row_idx == 0 && self.show_header {
                        format!("{} {}", header_text, col_idx + 1)
                    } else {
                        format!("{} [{},{}]", cell_text, row_idx + 1, col_idx + 1)
                    };
                    props.push(
                        ConfigProperty::text(&id, &label, cell_value)
                            .with_category(PropertyCategory::Text)
                            .with_order(3 + (row_idx * self.columns as usize + col_idx) as i32)
                    );
                }
            }
        }

        Some(props)
    }

    fn apply_text_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "rows_count" => {
                if let Some(v) = value.as_number() {
                    let new_rows = (v as usize).clamp(1, 20);
                    let cols = self.columns as usize;
                    while self.rows.len() < new_rows {
                        self.rows.push(vec!["".to_string(); cols]);
                    }
                    while self.rows.len() > new_rows {
                        self.rows.pop();
                    }
                    return true;
                }
            }
            "cols_count" => {
                if let Some(v) = value.as_number() {
                    let new_cols = (v as u8).clamp(1, 10);
                    self.columns = new_cols;
                    for row in &mut self.rows {
                        while row.len() < new_cols as usize {
                            row.push("".to_string());
                        }
                        while row.len() > new_cols as usize {
                            row.pop();
                        }
                    }
                    return true;
                }
            }
            "show_header" => {
                if let Some(v) = value.as_bool() {
                    self.show_header = v;
                    return true;
                }
            }
            _ if id.starts_with("cell_") => {
                let parts: Vec<&str> = id.strip_prefix("cell_").unwrap_or("").split('_').collect();
                if parts.len() == 2 {
                    if let (Ok(row), Ok(col)) = (parts[0].parse::<usize>(), parts[1].parse::<usize>()) {
                        if let Some(text) = value.as_string() {
                            while self.rows.len() <= row {
                                self.rows.push(vec!["".to_string(); self.columns as usize]);
                            }
                            while self.rows[row].len() <= col {
                                self.rows[row].push("".to_string());
                            }
                            self.rows[row][col] = text.to_string();
                            return true;
                        }
                    }
                }
            }
            _ => {}
        }
        false
    }

    fn style_properties(&self) -> Vec<ConfigProperty> {
        vec![
            ConfigProperty::header_color(&self.header_color)
                .with_category(PropertyCategory::Style)
                .with_order(10),
            ConfigProperty::grid_color(&self.grid_color)
                .with_category(PropertyCategory::Style)
                .with_order(11),
            ConfigProperty::text_color(&self.text_color)
                .with_category(PropertyCategory::Style)
                .with_order(12),
            ConfigProperty::header_text_color(&self.header_text_color)
                .with_category(PropertyCategory::Style)
                .with_order(13),
        ]
    }

    fn apply_style_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "header_color" => {
                if let Some(c) = value.as_color() {
                    self.header_color = c.to_string();
                    return true;
                }
            }
            "grid_color" => {
                if let Some(c) = value.as_color() {
                    self.grid_color = c.to_string();
                    return true;
                }
            }
            "text_color" => {
                if let Some(c) = value.as_color() {
                    self.text_color = c.to_string();
                    return true;
                }
            }
            "header_text_color" => {
                if let Some(c) = value.as_color() {
                    self.header_text_color = c.to_string();
                    return true;
                }
            }
            _ => {}
        }
        false
    }
}

pub fn metadata() -> PrimitiveMetadata {
    PrimitiveMetadata {
        type_id: "table", display_name: "Table", kind: PrimitiveKind::Annotation,
        click_behavior: ClickBehavior::TwoPoint, tooltip: "Data table", icon: "table", default_color: "#607D8B",
        factory: |points, color| { let (t1, p1) = points.first().copied().unwrap_or((0, 0.0)); let (t2, p2) = points.get(1).copied().unwrap_or((t1+600_000, p1-20.0)); Box::new(Table::new(t1, p1, t2, p2, color)) },
        supports_text: true,
        has_levels: false,
        has_points_config: false,
    }
}
