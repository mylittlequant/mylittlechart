//! Draws the chart into a rect through uzor's `RenderContext`: grid, candles, the
//! price axis on the right, the time axis below, the last price, the crosshair with
//! its labels and the OHLC line. Monochrome: greys only.

use mlc_core::{format_price, nice_price_step, Bar};
use uzor::render::{RenderContext, TextAlign, TextBaseline};
use uzor::Rect;

use crate::i18n::{t, K};
use crate::model::ChartModel;

/// Price axis width, px.
pub const AXIS_W: f64 = 62.0;
/// Time axis height, px.
pub const AXIS_H: f64 = 22.0;

/// The greys.
pub mod colors {
    pub const BG: &str = "#121212";
    pub const GRID: &str = "#1e1e1e";
    pub const AXIS_LINE: &str = "#262626";
    pub const TEXT: &str = "#8e8e8e";
    pub const UP: &str = "#d8d8d8";
    pub const DOWN: &str = "#5e5e5e";
    pub const LAST: &str = "#9a9a9a";
    pub const CROSS: &str = "#7a7a7a";
    pub const LABEL_BG: &str = "#e6e6e6";
    pub const LABEL_FG: &str = "#121212";
    pub const DIM: &str = "#5a5a5a";
}

/// Where the parts of the chart go inside its rect.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Areas {
    pub plot: Rect,
    pub price_axis: Rect,
    pub time_axis: Rect,
}

/// Split `r` into the plot, the price axis (right) and the time axis (bottom).
pub fn areas(r: Rect) -> Areas {
    let pw = (r.width - AXIS_W).max(0.0);
    let ph = (r.height - AXIS_H).max(0.0);
    Areas {
        plot: Rect::new(r.x, r.y, pw, ph),
        price_axis: Rect::new(r.x + pw, r.y, r.width - pw, ph),
        time_axis: Rect::new(r.x, r.y + ph, r.width, r.height - ph),
    }
}

fn y_of(p: f64, lo: f64, hi: f64, plot: Rect) -> f64 {
    plot.y + (hi - p) / (hi - lo) * plot.height
}

/// Price at plot-local y.
pub fn price_at(y: f64, lo: f64, hi: f64, h: f64) -> f64 {
    hi - y / h.max(1.0) * (hi - lo)
}

/// (year, month, day, hour, minute) of a unix time, UTC.
pub fn civil(ts: i64) -> (i64, u32, u32, u32, u32) {
    let days = ts.div_euclid(86_400);
    let secs = ts.rem_euclid(86_400);
    // Howard Hinnant's days-to-civil.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, (secs / 3600) as u32, (secs % 3600 / 60) as u32)
}

/// Axis label of a bar time: `HH:MM` inside a day, `DD.MM` at a day start or on
/// daily bars.
pub fn time_label(ts: i64, bar_secs: i64) -> String {
    let (_, mo, d, h, mi) = civil(ts);
    if bar_secs >= 86_400 || (h == 0 && mi == 0) { format!("{d:02}.{mo:02}") } else { format!("{h:02}:{mi:02}") }
}

/// Paint the chart of `m` into `r`. `label` is drawn faint in the plot's corner
/// (the demo note); the text uses `font` (family).
pub fn chart(cx: &mut dyn RenderContext, r: Rect, m: &ChartModel, label: Option<&str>) {
    let a = areas(r);
    cx.save();
    cx.begin_path();
    cx.rect(r.x, r.y, r.width, r.height);
    cx.clip();
    cx.set_fill_color(colors::BG);
    cx.fill_rect(r.x, r.y, r.width, r.height);
    let Some((lo, hi)) = m.price_range() else {
        cx.set_fill_color(colors::TEXT);
        cx.set_font("14px Inter, sans-serif");
        cx.set_text_align(TextAlign::Center);
        cx.set_text_baseline(TextBaseline::Middle);
        cx.fill_text(t(K::Loading), r.x + r.width / 2.0, r.y + r.height / 2.0);
        cx.restore();
        return;
    };
    let plot = a.plot;
    let bar_secs = match m.bars.as_slice() {
        [.., p, l] => (l.timestamp - p.timestamp).max(1),
        _ => 3600,
    };

    // Grid + price labels.
    let step = nice_price_step(hi - lo, (plot.height / 56.0).max(2.0));
    cx.set_font("11px Inter, sans-serif");
    cx.set_text_baseline(TextBaseline::Middle);
    cx.set_text_align(TextAlign::Left);
    cx.set_stroke_width(1.0);
    // Grid labels give way to the price tags drawn over the axis.
    let mut tags: Vec<f64> = Vec::new();
    if let Some(last) = m.bars.last() {
        tags.push(y_of(last.close, lo, hi, plot));
    }
    if let Some((_, y)) = m.crosshair {
        tags.push(plot.y + y);
    }
    let mut p = (lo / step).ceil() * step;
    while p <= hi {
        let y = y_of(p, lo, hi, plot).round() + 0.5;
        cx.set_stroke_color(colors::GRID);
        cx.begin_path();
        cx.move_to(plot.x, y);
        cx.line_to(plot.x + plot.width, y);
        cx.stroke();
        if y > plot.y + 8.0 && y < plot.y + plot.height - 8.0 && tags.iter().all(|t| (t - y).abs() > 16.0) {
            cx.set_fill_color(colors::TEXT);
            cx.fill_text(&format_price(p, step), a.price_axis.x + 8.0, y);
        }
        p += step;
    }

    // Time grid + labels: one every >= 72 px, on round bar indices.
    let bw = m.bar_w(plot.width);
    let every = ((72.0 / bw).ceil() as usize).max(1);
    let nice = [1usize, 2, 3, 4, 6, 8, 12, 16, 24, 32, 48, 64, 96, 128, 192, 256, 384, 512];
    let every = nice.iter().copied().find(|&s| s >= every).unwrap_or(every);
    cx.set_text_align(TextAlign::Center);
    let vis = m.visible();
    for i in vis.clone() {
        let b = &m.bars[i];
        if (b.timestamp / bar_secs) as usize % every != 0 {
            continue;
        }
        let x = (plot.x + m.x(i as f64, plot.width)).round() + 0.5;
        cx.set_stroke_color(colors::GRID);
        cx.begin_path();
        cx.move_to(x, plot.y);
        cx.line_to(x, plot.y + plot.height);
        cx.stroke();
        let near_cross = m.crosshair.is_some_and(|(cxp, _)| (plot.x + cxp - x).abs() < 64.0);
        if x > r.x + 18.0 && x < plot.x + plot.width - 18.0 && !near_cross {
            cx.set_fill_color(colors::TEXT);
            cx.fill_text(&time_label(b.timestamp, bar_secs), x, a.time_axis.y + a.time_axis.height / 2.0);
        }
    }

    // Axis borders.
    cx.set_stroke_color(colors::AXIS_LINE);
    cx.begin_path();
    cx.move_to(plot.x + plot.width + 0.5, plot.y);
    cx.line_to(plot.x + plot.width + 0.5, plot.y + plot.height);
    cx.move_to(r.x, plot.y + plot.height + 0.5);
    cx.line_to(r.x + r.width, plot.y + plot.height + 0.5);
    cx.stroke();

    // Candles, clipped to the plot.
    cx.save();
    cx.begin_path();
    cx.rect(plot.x, plot.y, plot.width, plot.height);
    cx.clip();
    let body_w = (bw * 0.7).max(1.0);
    for i in vis {
        let b = &m.bars[i];
        let x = plot.x + m.x(i as f64, plot.width);
        candle(cx, b, x, body_w, lo, hi, plot);
    }
    if let Some(l) = label {
        cx.set_font("12px Inter, sans-serif");
        cx.set_text_align(TextAlign::Right);
        cx.set_text_baseline(TextBaseline::Bottom);
        cx.set_fill_color(colors::DIM);
        cx.fill_text(l, plot.x + plot.width - 8.0, plot.y + plot.height - 6.0);
    }
    cx.restore();

    // Last price.
    if let Some(last) = m.bars.last() {
        let y = y_of(last.close, lo, hi, plot);
        if y >= plot.y && y <= plot.y + plot.height {
            cx.set_stroke_color(colors::LAST);
            cx.set_line_dash(&[3.0, 3.0]);
            cx.begin_path();
            cx.move_to(plot.x, y.round() + 0.5);
            cx.line_to(plot.x + plot.width, y.round() + 0.5);
            cx.stroke();
            cx.set_line_dash(&[]);
            axis_tag(cx, &format_price(last.close, step / 10.0), a.price_axis, y);
        }
    }

    // Crosshair, its labels and the OHLC line.
    let legend_bar = match m.crosshair {
        Some((x, y)) => {
            let hit = m.bar_at(x, plot.width);
            let sx = hit.map(|(i, _)| plot.x + m.x(i as f64, plot.width)).unwrap_or(plot.x + x);
            let sy = plot.y + y.clamp(0.0, plot.height);
            cx.set_stroke_color(colors::CROSS);
            cx.set_line_dash(&[4.0, 4.0]);
            cx.begin_path();
            cx.move_to(sx.round() + 0.5, plot.y);
            cx.line_to(sx.round() + 0.5, plot.y + plot.height);
            cx.move_to(plot.x, sy.round() + 0.5);
            cx.line_to(plot.x + plot.width, sy.round() + 0.5);
            cx.stroke();
            cx.set_line_dash(&[]);
            axis_tag(cx, &format_price(price_at(y, lo, hi, plot.height), step / 10.0), a.price_axis, sy);
            if let Some((_, b)) = hit {
                let txt = mlc_core::format_time_full(b.timestamp);
                cx.set_font("11px Inter, sans-serif");
                let w = cx.measure_text(&txt) + 12.0;
                let lx = (sx - w / 2.0).clamp(r.x, r.x + r.width - w);
                cx.set_fill_color(colors::LABEL_BG);
                cx.fill_rect(lx, a.time_axis.y + 2.0, w, a.time_axis.height - 4.0);
                cx.set_fill_color(colors::LABEL_FG);
                cx.set_text_align(TextAlign::Center);
                cx.set_text_baseline(TextBaseline::Middle);
                cx.fill_text(&txt, lx + w / 2.0, a.time_axis.y + a.time_axis.height / 2.0);
            }
            hit.map(|(_, b)| *b)
        }
        None => m.bars.last().copied(),
    };
    if let Some(b) = legend_bar {
        legend(cx, &b, step / 10.0, plot);
    }
    cx.restore();
}

fn candle(cx: &mut dyn RenderContext, b: &Bar, x: f64, body_w: f64, lo: f64, hi: f64, plot: Rect) {
    let up = b.close >= b.open;
    let c = if up { colors::UP } else { colors::DOWN };
    let (yo, yc) = (y_of(b.open, lo, hi, plot), y_of(b.close, lo, hi, plot));
    let (yh, yl) = (y_of(b.high, lo, hi, plot), y_of(b.low, lo, hi, plot));
    let xr = x.round() + 0.5;
    cx.set_stroke_color(c);
    cx.set_stroke_width(1.0);
    cx.begin_path();
    cx.move_to(xr, yh);
    cx.line_to(xr, yl);
    cx.stroke();
    let top = yo.min(yc);
    let h = (yo - yc).abs().max(1.0);
    cx.set_fill_color(c);
    cx.fill_rect((x - body_w / 2.0).round(), top, body_w.round().max(1.0), h);
}

fn axis_tag(cx: &mut dyn RenderContext, txt: &str, axis: Rect, y: f64) {
    let h = 18.0;
    let ty = (y - h / 2.0).clamp(axis.y, axis.y + axis.height - h);
    cx.set_fill_color(colors::LABEL_BG);
    cx.fill_rect(axis.x + 1.0, ty, axis.width - 1.0, h);
    cx.set_fill_color(colors::LABEL_FG);
    cx.set_font("11px Inter, sans-serif");
    cx.set_text_align(TextAlign::Left);
    cx.set_text_baseline(TextBaseline::Middle);
    cx.fill_text(txt, axis.x + 6.0, ty + h / 2.0);
}

fn legend(cx: &mut dyn RenderContext, b: &Bar, step: f64, plot: Rect) {
    cx.set_font("11px Inter, sans-serif");
    cx.set_text_align(TextAlign::Left);
    cx.set_text_baseline(TextBaseline::Top);
    let mut x = plot.x + 8.0;
    let y = plot.y + 6.0;
    for (k, v) in [(K::Open, b.open), (K::High, b.high), (K::Low, b.low), (K::Close, b.close)] {
        cx.set_fill_color(colors::DIM);
        let key = t(k);
        cx.fill_text(key, x, y);
        x += cx.measure_text(key) + 3.0;
        cx.set_fill_color(colors::TEXT);
        let val = format_price(v, step);
        cx.fill_text(&val, x, y);
        x += cx.measure_text(&val) + 8.0;
        if x > plot.x + plot.width - 40.0 {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil(0), (1970, 1, 1, 0, 0));
        assert_eq!(civil(1_762_905_600), (2025, 11, 12, 0, 0));
        assert_eq!(civil(951_782_400 + 3600 * 13 + 60 * 7), (2000, 2, 29, 13, 7));
        assert_eq!(time_label(1_762_905_600, 3600), "12.11");
        assert_eq!(time_label(1_762_905_600 + 5400, 900), "01:30");
    }

    #[test]
    fn areas_split_the_rect() {
        let a = areas(Rect::new(0.0, 100.0, 390.0, 500.0));
        assert_eq!(a.plot, Rect::new(0.0, 100.0, 390.0 - AXIS_W, 500.0 - AXIS_H));
        assert_eq!(a.price_axis.x + a.price_axis.width, 390.0);
        assert_eq!(a.time_axis.y + a.time_axis.height, 600.0);
    }
}
