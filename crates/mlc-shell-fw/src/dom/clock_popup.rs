//! The clock timezone/format popup as uzor DOM (Wave A): a right-anchored popup
//! with the UTC offset list (+12..-12) and the use-24h / show-UTC / glitch
//! checkboxes. Hit ids are the legacy `clock_popup:*` ones; the host forwards
//! picks to the app dispatcher.

use uzor::ui::dom::{Control, El};
use uzor::ui::domkit;

const POPUP_WIDTH: f64 = 200.0;
const HEADER_H: f64 = 36.0;
const ITEM_H: f64 = 24.0;
const SEPARATOR_H: f64 = 13.0;
const CHECKBOX_AREA_H: f64 = 24.0;
const TZ_COUNT: f64 = 25.0;
const POPUP_HEIGHT: f64 = HEADER_H + TZ_COUNT * ITEM_H + SEPARATOR_H + CHECKBOX_AREA_H * 3.0;

fn pen_label(text: &str, px: f64, row_h: f64) -> El {
    let line_h = 16.0;
    let base_in = uzor::ui::css_text::baseline(px, line_h, uzor::ui::css_text::INTER);
    El::new("span").css(format!("font-size: {px}px; line-height: {line_h}px; margin-top: {}px", row_h / 2.0 - base_in)).text(text)
}

/// Build the popup tree anchored above the bottom toolbar at the right edge.
pub fn tree(screen_w: f64, bottom_toolbar_y: f64, active_offset: i32, use_24h: bool, show_utc: bool, clock_glitch: bool) -> El {
    let x = screen_w - POPUP_WIDTH;
    let y = (bottom_toolbar_y - POPUP_HEIGHT).max(0.0);
    let tz_rows = (-12..=12_i32).rev().map(|offset| {
        let city = mlc_core::scale_settings::TimeFormatSettings::city_for_offset(offset);
        let label = if offset >= 0 { format!("UTC+{offset}  {city}") } else { format!("UTC{offset}  {city}") };
        let cls = if offset == active_offset { "div.k-tr.on" } else { "div.k-tr" };
        El::new(cls).id(format!("clock_popup:tz:{offset}")).tab().css(format!("height: {ITEM_H}px; align-items: flex-start")).child(pen_label(&label, 12.0, ITEM_H))
    });
    let cb = |id: &str, on: bool, label: &str| {
        El::new("div.k-check")
            .id(id)
            .tab()
            .css(format!("height: {CHECKBOX_AREA_H}px"))
            .child(El::new("span.k-box").control(Control::checkbox(on)))
            .child(pen_label(label, 12.0, CHECKBOX_AREA_H))
    };
    El::new("div.k-modal")
        .child(El::new("div").id("clock_popup:bg").css("position: absolute; left: 0px; top: 0px; width: 100000px; height: 100000px"))
        .child(
            El::new("div.k-dialog")
                .css(format!("top: {y}px; left: {x}px; width: {POPUP_WIDTH}px; height: {POPUP_HEIGHT}px"))
                .child(El::new("div.k-dialog-head").css(format!("height: {HEADER_H}px")).child(El::new("div.k-dialog-title").text("Timezone")))
                .child(El::new("div.k-tbody").children(tz_rows))
                .child(El::new("div.k-sep").css(format!("height: {SEPARATOR_H}px")))
                .child(cb("clock_popup:clock:use_24h", use_24h, "24-hour"))
                .child(cb("clock_popup:clock:show_utc", show_utc, "Show UTC"))
                .child(cb("clock_popup:clock:glitch", clock_glitch, "Glitch")),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_popup_tree_hits() {
        let el = tree(1280.0, 780.0, 0, true, false, false);
        let mut ctx = uzor_render_tiny_skia::TinySkiaCpuRenderContext::new(1280, 800, 1.0);
        let sheet = domkit::sheet();
        let dom = uzor::ui::dom::lay_out_in(&mut ctx, &sheet, &el, 0.0, 0.0, 1280.0, None, &|_| uzor::ui::style::State::NONE);
        let ids: Vec<&str> = dom.hits().map(|(i, _)| i).collect();
        for want in ["clock_popup:bg", "clock_popup:tz:0", "clock_popup:tz:12", "clock_popup:tz:-12", "clock_popup:clock:use_24h", "clock_popup:clock:show_utc", "clock_popup:clock:glitch"] {
            assert!(ids.contains(&want), "missing {want}");
        }
    }
}
