//! Shared-chart row — drawn on a read-only viewer tab's chrome
//! (`docs/mlc/plans/native-share-viewer-and-chart-rights-2026-09-06.md`
//! §1/§3, step 3 "Web shell").
//!
//! Pure presentation, the same split every modal in this crate follows
//! (`layout::modals::cloud_login`'s own module doc): this file draws the
//! row and registers ONE `share:clone:<preset_id>` widget id when the
//! caller says cloning is allowed; the click itself resolves through
//! `ChartApp::dispatch_panel_click` (`mlc-app`) the same way every other
//! `<prefix>:<id>` widget in this crate does, ending in
//! `ChartApp::request_clone_share(token)`. This module owns none of that
//! state — the caller (mlc-app's tab-chrome render, which holds
//! `ChartApp::shared_chart_meta`/`is_readonly_tab`) supplies the owner
//! label, the preset id and whether the link's rights include `"clone"`
//! on every frame.
//!
//! Not wired into a live render call yet: the call site lives in `mlc-app`'s
//! own chrome rendering, out of this slice's scope (concurrent work already
//! in flight there) — this is the ready-to-call primitive plus the
//! `share:clone:<preset_id>` hit-id contract the dispatch side (also
//! `mlc-app`) needs to match on.

use mlc_core::engine::render::{RenderContext, TextAlign, TextBaseline};
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::types::Rect as WidgetRect;
use uzor::WidgetId;

use crate::ui::toolbar_theme::ToolbarTheme;

/// Horizontal padding inside the "Clone to my charts" button.
const BTN_PAD_X: f64 = 12.0;
/// Corner radius, matching `ui::widgets::button::SETTINGS_BUTTON_RADIUS`.
const BTN_RADIUS: f64 = 4.0;

/// Draw the shared-chart row: "Shared by `<owner_label>`" plus a "Clone to
/// my charts" button when `can_clone` is true.
///
/// `preset_id` names the read-only tab this row belongs to — it becomes the
/// tail of the button's widget id (`share:clone:<preset_id>`) so a host with
/// several tabs open at once (native) can tell which tab's Clone was
/// pressed; a single-tab host (web) always passes its one active preset id.
///
/// Draws nothing and registers no widget when `can_clone` is false — a link
/// without the clone right, or a caller the effective-rights answer did not
/// grant it to (an anonymous viewer per the server's rights-v1 rule), never
/// sees the button at all; the 403 the server would otherwise answer is
/// never reachable from a UI that never offered the action.
pub fn draw_share_viewer_row(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    rect: WidgetRect,
    preset_id: &str,
    owner_label: &str,
    can_clone: bool,
    tt: &ToolbarTheme,
) {
    let label = format!("Shared by {owner_label}");
    ctx.set_font("13px sans-serif");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(&label, rect.x, rect.center_y());

    if !can_clone {
        return;
    }

    let btn_label = "Clone to my charts";
    ctx.set_font("600 13px sans-serif");
    let btn_w = ctx.measure_text(btn_label) + BTN_PAD_X * 2.0;
    let btn_rect = WidgetRect::new(rect.x + rect.width - btn_w, rect.y, btn_w, rect.height);

    let widget_id = format!("share:clone:{preset_id}");
    let hovered = coord.is_hovered(&WidgetId::from(widget_id.as_str()));
    let bg = if hovered { tt.button_bg_hover.as_str() } else { tt.button_bg.as_str() };
    ctx.set_fill_color(bg);
    ctx.fill_rounded_rect(btn_rect.x, btn_rect.y, btn_rect.width, btn_rect.height, BTN_RADIUS);
    ctx.set_fill_color(&tt.item_text);
    ctx.set_text_align(TextAlign::Center);
    ctx.fill_text(btn_label, btn_rect.center_x(), btn_rect.center_y());

    coord.register_on_layer(widget_id.as_str(), btn_rect, Sense::CLICK | Sense::HOVER, layer_id);
}
