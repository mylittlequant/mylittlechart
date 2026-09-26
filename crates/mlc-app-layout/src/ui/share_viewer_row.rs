//! Shared-chart row — drawn on a read-only viewer tab's chrome
//! (`docs/mlc/plans/native-share-viewer-and-chart-rights-2026-09-06.md`
//! §1/§3, step 3 "Web shell").
//!
//! Pure presentation, the same split every modal in this crate follows
//! (`layout::modals::cloud_login`'s own module doc): this file draws ONE
//! "Clone to my charts" button and registers its `share:clone:<preset_id>`
//! widget id when the caller says cloning is allowed; the click itself
//! resolves through `ChartApp::dispatch_panel_click` (`mlc-app`) the same
//! way every other `<prefix>:<id>` widget in this crate does, ending in
//! `ChartApp::request_clone_share(token)`. This module owns none of that
//! state — the caller (`mlc-app`'s `render_modal_layer`, which holds
//! `ChartApp::shared_chart_meta`/`is_readonly_tab`) supplies the preset id
//! and whether the link's rights include `"clone"` on every frame, plus a
//! rect already trimmed to the active leaf's tab-header band (owner ruling
//! 2026-09-24 — no more "Shared by `<owner>`" text; the tab title already
//! names the owner, and the label used to overlap the leaf's own legend).

use mlc_core::engine::render::{RenderContext, TextAlign, TextBaseline};
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::types::Rect as WidgetRect;
use uzor::WidgetId;

use crate::ui::toolbar_theme::ToolbarTheme;

/// Horizontal padding inside the "Clone to my charts" button.
const BTN_PAD_X: f64 = 12.0;
/// Corner radius, matching `ui::widgets::button::SETTINGS_BUTTON_RADIUS`.
const BTN_RADIUS: f64 = 4.0;

/// Draw the shared-chart row: a compact "Clone to my charts" button, when
/// `can_clone` is true — nothing otherwise.
///
/// No "Shared by `<owner>`" label (owner ruling 2026-09-24: the tab title
/// already names the owner; the label used to sit in the same band as the
/// leaf's legend and overlap it on a read-only shared tab). `rect` is the
/// caller's leaf-tab header band with the price-scale width already
/// subtracted off its right edge — this draws only the button, right-aligned
/// within `rect`, so it never reaches into the price-scale labels either.
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
    can_clone: bool,
    tt: &ToolbarTheme,
) {
    if !can_clone {
        return;
    }

    let btn_label = "Clone to my charts";
    ctx.set_font("600 12px sans-serif");
    let btn_w = ctx.measure_text(btn_label) + BTN_PAD_X * 2.0;
    let btn_rect = WidgetRect::new(rect.x + rect.width - btn_w, rect.y, btn_w, rect.height);

    let widget_id = format!("share:clone:{preset_id}");
    let hovered = coord.is_hovered(&WidgetId::from(widget_id.as_str()));
    let bg = if hovered { tt.button_bg_hover.as_str() } else { tt.button_bg.as_str() };
    ctx.set_fill_color(bg);
    ctx.fill_rounded_rect(btn_rect.x, btn_rect.y, btn_rect.width, btn_rect.height, BTN_RADIUS);
    ctx.set_fill_color(&tt.item_text);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(btn_label, btn_rect.center_x(), btn_rect.center_y());

    coord.register_on_layer(widget_id.as_str(), btn_rect, Sense::CLICK | Sense::HOVER, layer_id);
}

/// Author's live controls, same corner as the viewer's Clone button.
/// Live is the socket. View and Cursor ride that socket and drop off with it.
pub fn draw_share_live_controls(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    rect: WidgetRect,
    live: bool,
    viewport: bool,
    cursor: bool,
    tt: &ToolbarTheme,
) {
    let mut buttons = vec![("share_live:live", "Live", live)];
    if live {
        buttons.push(("share_live:viewport", "View", viewport));
        buttons.push(("share_live:cursor", "Cursor", cursor));
    }
    ctx.set_font("600 12px sans-serif");
    let gap = 4.0;
    let mut right = rect.x + rect.width;
    for (id, label, on) in buttons.into_iter().rev() {
        let w = ctx.measure_text(label) + BTN_PAD_X * 2.0;
        right -= w;
        let btn = WidgetRect::new(right, rect.y, w, rect.height);
        right -= gap;
        let hovered = coord.is_hovered(&WidgetId::from(id));
        let bg = if on {
            tt.accent.as_str()
        } else if hovered {
            tt.button_bg_hover.as_str()
        } else {
            tt.button_bg.as_str()
        };
        ctx.set_fill_color(bg);
        ctx.fill_rounded_rect(btn.x, btn.y, btn.width, btn.height, BTN_RADIUS);
        ctx.set_fill_color(if on { "#ffffff" } else { tt.item_text.as_str() });
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, btn.center_x(), btn.center_y());
        coord.register_on_layer(id, btn, Sense::CLICK | Sense::HOVER, layer_id);
    }
}

/// The author turned Live off. The chart stays put until the viewer presses
/// the button, which closes this tab.
pub fn draw_stream_ended(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    rect: WidgetRect,
    title: &str,
    action: &str,
    chart_bg: &str,
    tt: &ToolbarTheme,
) {
    ctx.set_fill_color(chart_bg);
    ctx.fill_rect(rect.x, rect.y, rect.width, rect.height);
    coord.register_on_layer("share_end:bg", rect, Sense::CLICK, layer_id);

    let card_w = 320.0_f64.min(rect.width - 24.0).max(160.0);
    let card_h = 120.0_f64.min(rect.height - 24.0).max(88.0);
    let card = WidgetRect::new(
        rect.x + (rect.width - card_w) / 2.0,
        rect.y + (rect.height - card_h) / 2.0,
        card_w,
        card_h,
    );
    ctx.set_fill_color(chart_bg);
    ctx.fill_rounded_rect(card.x, card.y, card.width, card.height, 8.0);

    ctx.set_font("600 16px sans-serif");
    ctx.set_fill_color(&tt.item_text);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(title, card.center_x(), card.y + 36.0);

    ctx.set_font("600 13px sans-serif");
    let btn_w = ctx.measure_text(action) + 28.0;
    let btn = WidgetRect::new(card.center_x() - btn_w / 2.0, card.y + card.height - 48.0, btn_w, 32.0);
    let hovered = coord.is_hovered(&WidgetId::from("share_end:close"));
    ctx.set_fill_color(if hovered { tt.accent_hover.as_str() } else { tt.accent.as_str() });
    ctx.fill_rounded_rect(btn.x, btn.y, btn.width, btn.height, BTN_RADIUS);
    ctx.set_fill_color("#ffffff");
    ctx.fill_text(action, btn.center_x(), btn.center_y());
    coord.register_on_layer("share_end:close", btn, Sense::CLICK | Sense::HOVER, layer_id);
}
