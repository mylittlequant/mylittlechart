//! The chart browser (preset picker) as uzor DOM (Wave A): a dialog with a search
//! field, an "open in new tab" toggle and the preset list (row / rename / delete
//! hits). Hit ids are the legacy `chart_browser:*` ones so the app dispatcher
//! handles picks unchanged; the host forwards `resp.clicked` to it.

use mlc_app_layout::ui::modal_settings::ChartBrowserState;
use uzor::ui::dom::{Control, El};
use uzor::ui::domkit;

/// Build the browser tree. `presets` = (id, name) pairs already filtered by the
/// app's search query.
pub fn tree(s: &ChartBrowserState, presets: &[(String, String)]) -> El {
    let (w, h) = (520.0, 420.0);
    let (x, y) = s.position.unwrap_or((640.0 - w / 2.0, 400.0 - h / 2.0));
    let rows = presets.iter().map(|(id, name)| {
        El::new("div.k-tr")
            .id(format!("chart_browser:item:{id}"))
            .tab()
            .child(El::new("div.k-td").text(name.clone()))
            .child(El::new("button.k-act").id(format!("chart_browser:rename:{id}")).tab().name("Rename").text("✎"))
            .child(El::new("button.k-act").id(format!("chart_browser:delete:{id}")).tab().name("Delete").text("🗑"))
    });
    let list = El::new("div.k-tbody").css("overflow: auto; max-height: 280px").children(rows);
    El::new("div.k-modal")
        .child(El::new("div.k-scrim").id("chart_browser:modal_bg"))
        .child(
            El::new("div.k-dialog")
                .css(format!("top: {y}px; left: {x}px; width: {w}px; height: {h}px"))
                .child(
                    El::new("div.k-dialog-head")
                        .child(El::new("div.k-dialog-title").text("Chart Browser"))
                        .child(El::new("button.k-act").id("chart_browser:close").tab().name("Close").svg(domkit::icon(domkit::icons::CLOSE))),
                )
                .child(
                    El::new("div.k-dialog-body")
                        .css("display: flex; flex-direction: column; gap: 8px; padding: 10px")
                        .child(El::new("div.k-input").id("chart_browser:search_input").control(Control::input(uzor::ui::dom::Edit::new(s.search_query.clone()).placeholder("Search presets…"))))
                        .child(
                            El::new(if s.open_in_new_tab { "div.k-check" } else { "div.k-check" })
                                .id("chart_browser:new_tab")
                                .tab()
                                .child(El::new("span.k-box").control(Control::checkbox(s.open_in_new_tab)))
                                .child(El::new("span").text("Open in new tab")),
                        )
                        .child(list),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_browser_tree_hits() {
        let s = ChartBrowserState::default();
        let presets = vec![("p1".to_string(), "Preset One".to_string())];
        let el = tree(&s, &presets);
        let mut ctx = uzor_render_tiny_skia::TinySkiaCpuRenderContext::new(1280, 800, 1.0);
        let sheet = domkit::sheet();
        let dom = uzor::ui::dom::lay_out_in(&mut ctx, &sheet, &el, 0.0, 0.0, 1280.0, None, &|_| uzor::ui::style::State::NONE);
        let ids: Vec<&str> = dom.hits().map(|(i, _)| i).collect();
        for want in ["chart_browser:close", "chart_browser:item:p1", "chart_browser:rename:p1", "chart_browser:delete:p1", "chart_browser:new_tab"] {
            assert!(ids.contains(&want), "missing {want}");
        }
    }
}
