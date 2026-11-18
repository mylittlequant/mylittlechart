//! The Community modal (Forum + Messages) as uzor DOM (item 15).
//!
//! Was: `layout::modals::community::render_community_modal`, hand-drawn
//! through `begin_modal_shell`/`end_modal_shell`. Now the paint is this
//! tree: the frame/header are `domkit::dialog`-shaped boxes calibrated to
//! the legacy shell's pixels (no footer — the pane's composer is the final
//! action row; no dim veil — `dim_backdrop` is off for this modal), the
//! Forum/Messages header pills paint into an embed slot over the header
//! row, the active pane into a second embed slot at the shell content
//! origin, and the resize corner hint into a third (the legacy
//! `end_modal_shell` grips, footerless).
//!
//! In place: the app still lays the modal out, registers its hit zones and
//! runs its input — its paint goes nowhere (`ChartPanelApp::community_on_dom`,
//! `PaintOff`). The embed slots carry no widget id, so pane clicks fall
//! through to the app's live zones; the close button is the one DOM hit,
//! forwarded under its legacy id ([`app_id`]). The Messages tab reads the
//! outside `MessengerProvider` through the same `messenger_provider_override`
//! argument the legacy renderer takes — the host passes
//! `ChartPanelApp::messenger_provider_view()`.

use uzor::input::{InputCoordinator, LayerId};
use uzor::render::Painter;
use uzor::types::Rect;
use uzor::ui::dom::El;
use uzor::ui::domkit;

use crate::layout::modals::community::{community_spec, draw_pane_dom, draw_pills_dom, CommunityResult};
use crate::layout::FrameTheme;
use crate::ui::modal_settings::CommunityState;
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{end_modal_shell, ModalShell, ModalTheme};

use super::{fixed, geometry, SlotChartCtx};

/// The dialog id namespace — the legacy one, so DOM hits ARE legacy ids.
pub const ID: &str = "community";
/// The active pane's embed paint slot (no widget id: clicks stay the app's).
pub const BODY: &str = "community:body";
/// The Forum/Messages header pills' embed paint slot.
pub const PILLS: &str = "community:pills";
/// The resize corner hint's embed paint slot (legacy `end_modal_shell`).
pub const GRIP: &str = "community:grip";

/// Theme colours, as the legacy shell resolved them.
#[derive(Clone, Debug)]
pub struct Look {
    pub theme: ModalTheme,
}

impl Look {
    pub fn new(ft: &FrameTheme, tt: &ToolbarTheme) -> Self {
        Look { theme: ModalTheme::from_toolbar(&ft.modal_bg, &tt.separator, tt) }
    }

    /// The kit dialog rules, calibrated to the legacy shell (same chrome
    /// constants as the cloud overlay's DOM shell: 44 px header, square
    /// corners, the 1 px stroke painted by [`super::frame_border`]). No
    /// footer: this modal carries none.
    pub fn css(&self) -> String {
        let t = &self.theme;
        format!(
            "div.m-root {{ font-family: sans-serif }}
div.k-scrim {{ background: transparent }}
div.k-dialog {{ border-radius: 0; background: transparent; border: 2px solid transparent; box-shadow: none }}
div.k-dialog-head, div.k-dialog-body {{ background: transparent }}
div.k-dialog-head {{ height: 43px; flex-shrink: 0; padding: 0 6px 0 15px; font-size: 13px; font-weight: 400; color: {text}; border-bottom: 1px solid transparent }}
div.k-dialog-head > div.k-dialog-title {{ padding-top: 1.8px }}
div.k-dialog-head > button.k-act {{ color: {text} }}
div.k-dialog-head > button.k-act:hover {{ background: transparent; color: {close_hover} }}
div.k-dialog-body {{ min-height: 0; flex-shrink: 1; padding: 0; gap: 0; position: relative }}",
            text = t.header_text,
            close_hover = t.close_button_hover,
        )
    }
}

/// The dialog's DOM rect: the legacy frame one pixel larger all round, for
/// the half-alpha border that models the 1 px stroke's AA spread.
pub fn frame(window: Rect, chrome_h: f64, state: &CommunityState) -> Rect {
    let g = geometry(&community_spec(state, false), window, chrome_h);
    Rect::new(g.frame.x - 1.0, g.frame.y - 1.0, g.frame.width + 2.0, g.frame.height + 2.0)
}

/// The whole tree. `at` is [`frame`]'s rect; `window`/`chrome_h` place the
/// fixed boxes (frame fill, shadow, hairline, pane/pill/grip slots) in
/// window coordinates.
pub fn tree(state: &CommunityState, look: &Look, at: Rect, window: Rect, chrome_h: f64) -> El {
    let g = geometry(&community_spec(state, false), window, chrome_h);
    let t = &look.theme;
    // The kit dialog's shape, without ids on the frame box or the scrim:
    // an id on the frame would swallow the pane clicks that belong to the
    // app's live zones, and an id on the scrim would swallow the
    // click-outside that the app's own modal-layer path dismisses on.
    let head = El::new("div.k-dialog-head")
        .child(El::new("div.k-dialog-title").text("Community"))
        .child(
            El::new("button.k-act")
                .id(format!("{ID}:close"))
                .tab()
                .name("Close")
                .svg(domkit::icon(domkit::icons::CLOSE)),
        );
    let dialog = El::new("div.k-modal")
        .child(El::new("div.k-scrim"))
        .child(
            El::new("div.k-dialog")
                .css(format!("top: {}px; left: {}px; width: {}px; height: {}px", at.y, at.x, at.width, at.height))
                .child(head)
                .child(El::new("div.k-dialog-body")),
        );
    let shadow = fixed("cm-shadow", 145, Rect::new(g.frame.x + 3.0, g.frame.y + 3.0, g.frame.width, g.frame.height), "background: rgba(0,0,0,0.4)");
    let bg = fixed("cm-bg", 149, g.frame, &format!("background: {}", t.background));
    // The pills paint over the header row (legacy `draw_header_tabs` draws
    // at the header's own coordinates; the slot box contains them). +1 px
    // tall for the row's edge AA.
    let pills = fixed("cm-slot", 152, Rect::new(g.header.x, g.header.y, g.header.width, g.header.height + 1.0), "").slot(PILLS);
    let body = fixed("cm-slot", 152, Rect::new(g.content.x, g.content.y, g.content.width + 1.0, g.content.height + 1.0), "").slot(BODY);
    // The resize hint: legacy strokes reach 9 px in from the corner; the
    // slot box carries them plus their edge AA.
    let grip = fixed(
        "cm-slot",
        151,
        Rect::new(g.frame.right() - 10.0, g.frame.bottom() - 10.0, 11.0, 11.0),
        "",
    )
    .slot(GRIP);
    El::new("div.k-root.m-root")
        .css("height: 100%")
        .child(dialog)
        .child(shadow)
        .child(bg)
        .children(super::frame_border(g.frame, &t.border))
        .children(super::hairline(g.frame.x, g.frame.y + 44.0, g.frame.right(), &t.header_border))
        .child(pills)
        .child(body)
        .child(grip)
}

/// Paint one embed slot (`key` is [`BODY`]/[`PILLS`]/[`GRIP`]): the legacy
/// renderer, in the app's own chart coordinate space, at the shell geometry
/// [`geometry`] settled on. `provider` is the outside `MessengerProvider`
/// the Messages tab reads (`ChartPanelApp::messenger_provider_view()`), the
/// same override `render_community_modal` takes. Hits go to a throwaway
/// coordinator — the app's layout pass registered the live zones.
#[allow(clippy::too_many_arguments)]
pub fn paint_slot(
    ctx: &mut dyn uzor::render::RenderContext,
    key: &str,
    state: &CommunityState,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    provider: Option<&dyn mlc_messenger_model::MessengerProvider>,
    window: Rect,
    chrome_h: f64,
) {
    let spec = community_spec(state, false);
    let g = geometry(&spec, window, chrome_h);
    let (ox, oy) = (window.x, window.y + chrome_h);
    let chart = |r: Rect| Rect::new(r.x - ox, r.y - oy, r.width, r.height);
    let shell = ModalShell {
        frame: chart(g.frame),
        header: chart(g.header),
        content: chart(g.content),
        overflow: g.overflow,
        tabs: g.tabs.map(chart),
        tab_rects: g.tab_rects.into_iter().map(|(id, r)| (id, chart(r))).collect(),
        close_btn: g.close_btn.map(chart),
        footer: g.footer.map(chart),
        template_btn: g.template_btn.map(chart),
        primary_btn: g.primary_btn.map(chart),
    };
    let theme = ModalTheme::from_toolbar(&ft.modal_bg, &tt.separator, tt);
    let mut coord = InputCoordinator::default();
    let layer = LayerId::new("community-dom-body");
    let mut result = CommunityResult::default();
    let mut c = SlotChartCtx(ctx);
    c.save();
    c.translate(ox, oy);
    match key {
        PILLS => draw_pills_dom(&mut c, &mut coord, &layer, &shell, state, tt, provider, &mut result),
        BODY => draw_pane_dom(&mut c, &mut coord, &layer, &shell, state, ft, tt, current_time_ms, provider, &mut result),
        // Footerless: `end_modal_shell` paints the resize grips alone.
        GRIP => end_modal_shell(&mut c, &mut coord, &spec, &theme, &shell, &layer, &[]),
        _ => {}
    }
    c.restore();
}

/// What a DOM click means to the app: the close button already carries its
/// legacy id; anything else under the namespace passes through unchanged.
pub fn app_id(clicked: &str) -> Option<String> {
    clicked.starts_with("community:").then(|| clicked.to_string())
}
