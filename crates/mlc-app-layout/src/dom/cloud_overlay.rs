//! The cloud News/Links/Download overlay as uzor DOM (item 15).
//!
//! Was: `layout::modals::cloud_overlay::render_cloud_overlay`, hand-drawn
//! through `render_modal_shell` over a dimmed backdrop. Now the paint is
//! this tree: the dim veil is a fixed box over the CHART AREA (the chrome
//! strip keeps the shell's own `render_backdrop_dim` veil — see [`tree`]),
//! the frame/header/footer are `domkit::dialog`-shaped boxes calibrated to
//! the legacy shell's pixels, and the legacy body (whichever view the state
//! names) paints into an embed slot at the shell content origin. The News
//! list view's tab bar is the shell's own `draw_tabs` pixels in a second
//! embed slot.
//!
//! In place: the app still lays the overlay out, registers its hit zones
//! (the dimmer strips included) and runs its input — its paint goes nowhere
//! (`ChartPanelApp::cloud_overlay_on_dom`, `PaintOff`). The embed slots
//! carry no widget id, so body clicks fall through to the app's live zones;
//! only the shell's own controls (close, OK, click-outside) are DOM hits,
//! forwarded under the legacy ids ([`app_id`]).

use uzor::input::{InputCoordinator, LayerId};
use uzor::render::Painter;
use uzor::types::Rect;
use uzor::ui::dom::El;
use uzor::ui::domkit;

use crate::layout::modals::cloud_overlay::{
    draw_body_dom, draw_news_tabs, overlay_spec, CloudOverlayResult,
};
use crate::layout::FrameTheme;
use crate::ui::modal_settings::CloudOverlayState;
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{overlay_backdrop, ModalShell, ModalTheme};

use super::{fixed, geometry, SlotChartCtx};

/// The dialog id namespace — the legacy one, so DOM hits ARE legacy ids.
pub const ID: &str = "cloud_overlay";
/// The legacy body's embed paint slot (no widget id: clicks stay the app's).
pub const BODY: &str = "cloud_overlay:body";
/// The News tab bar's embed paint slot.
pub const TABS: &str = "cloud_overlay:tabs";

/// Theme colours, as the legacy shell resolved them (`ModalTheme::from_toolbar`
/// plus the dim veil `overlay_backdrop` picks for the same modal background).
#[derive(Clone, Debug)]
pub struct Look {
    pub theme: ModalTheme,
    pub backdrop: String,
}

impl Look {
    pub fn new(ft: &FrameTheme, tt: &ToolbarTheme) -> Self {
        Look {
            theme: ModalTheme::from_toolbar(&ft.modal_bg, &tt.separator, tt),
            backdrop: overlay_backdrop(&ft.modal_bg),
        }
    }

    /// The kit dialog rules, calibrated to the legacy shell: 44 px header
    /// (43 + the hairline row), 52 px footer with the 70×32 OK hard against
    /// the right inset, square corners. The frame's 1 px stroke is painted
    /// by [`super::frame_border`] (half-alpha edge strips + quarter-alpha
    /// miter corners), not by a CSS border; the dialog's own 2 px border is
    /// transparent layout only. The frame fill is a fixed box UNDER the
    /// dialog ([`tree`]), so it covers the exact legacy rect instead of the
    /// border box's content inset.
    pub fn css(&self) -> String {
        let t = &self.theme;
        format!(
            "div.m-root {{ font-family: sans-serif }}
div.k-scrim {{ background: transparent }}
div.k-dialog {{ border-radius: 0; background: transparent; border: 2px solid transparent; box-shadow: none }}
div.k-dialog-head, div.k-dialog-body, div.k-dialog-foot {{ background: transparent }}
div.k-dialog-head {{ height: 43px; flex-shrink: 0; padding: 0 6px 0 15px; font-size: 13px; font-weight: 400; color: {text}; border-bottom: 1px solid transparent }}
div.k-dialog-head > div.k-dialog-title {{ padding-top: 1.8px }}
div.k-dialog-head > button.k-act {{ color: {text} }}
div.k-dialog-head > button.k-act:hover {{ background: transparent; color: {close_hover} }}
div.k-dialog-body {{ min-height: 0; flex-shrink: 1; padding: 0; gap: 0; position: relative }}
div.k-dialog-foot {{ height: 52px; flex-shrink: 0; padding: 0 15px; align-items: flex-start; justify-content: flex-end; border-top: 2px solid transparent; position: relative }}
div.k-dialog-foot > button.k-btn.primary {{ margin-top: 9px; width: 70px; height: 32px; padding: 2px 0 0 0; border: none; border-radius: 0; font-size: 13px; font-weight: 400; background: {accent}; color: {accent_text}; text-align: center }}
div.k-dialog-foot > button.k-btn.primary:hover {{ background: {accent_hover} }}",
            text = t.header_text,
            close_hover = t.close_button_hover,
            accent = t.accent,
            accent_text = t.accent_text,
            accent_hover = t.accent_hover,
        )
    }
}

/// The dialog's DOM rect: the legacy frame one pixel larger all round, for
/// the half-alpha border that models the 1 px stroke's AA spread.
pub fn frame(window: Rect, chrome_h: f64, state: &CloudOverlayState) -> Rect {
    let g = geometry(&overlay_spec(state), window, chrome_h);
    Rect::new(g.frame.x - 1.0, g.frame.y - 1.0, g.frame.width + 2.0, g.frame.height + 2.0)
}

/// The whole tree. `at` is [`frame`]'s rect; `window`/`chrome_h` place the
/// fixed boxes (frame fill, shadow, hairlines, body/tab slots) in window
/// coordinates.
pub fn tree(state: &CloudOverlayState, look: &Look, at: Rect, window: Rect, chrome_h: f64) -> El {
    let g = geometry(&overlay_spec(state), window, chrome_h);
    let t = &look.theme;
    let title = overlay_spec(state).title;
    // The kit dialog's shape, without its id on the frame box: an id there
    // would register a CLICK widget over the whole dialog and swallow the
    // body clicks that belong to the app's live zones.
    let head = El::new("div.k-dialog-head")
        .child(El::new("div.k-dialog-title").text(title))
        .child(
            El::new("button.k-act")
                .id(format!("{ID}:close"))
                .tab()
                .name("Close")
                .svg(domkit::icon(domkit::icons::CLOSE)),
        );
    let foot = El::new("div.k-dialog-foot").child(
        El::new("button.k-btn.primary").id(format!("{ID}:foot:0")).tab().text("OK"),
    );
    // The scrim carries NO id: an id would register a full-window CLICK
    // widget that swallows the body clicks and the dimmer dismissals the
    // app's own live zones own (the legacy dimmer strips are registered by
    // the app's layout pass, exactly as before).
    let dialog = El::new("div.k-modal")
        .child(El::new("div.k-scrim"))
        .child(
            El::new("div.k-dialog")
                .css(format!("top: {}px; left: {}px; width: {}px; height: {}px", at.y, at.x, at.width, at.height))
                .child(head)
                .child(El::new("div.k-dialog-body"))
                .child(foot),
        );
    // The legacy frame's own pixels, under the dialog border: the shadow
    // (+3, radius 0), the fill at the EXACT frame rect, then the header and
    // footer hairlines as 2 px half-alpha rows centred on the stroked edge.
    // The dim veil covers the CHART AREA only — the legacy overlay's own
    // backdrop fill is `(0, 0, screen_w, screen_h)` in chart space, and the
    // chrome strip is dimmed separately by the shell's `paint_over`
    // (`render_backdrop_dim`, which leaves the native system buttons live).
    // A whole-window veil here would stack a SECOND dim over that strip:
    // the item-15 blocker's ~27k px divergence was exactly this double veil
    // (measured: 6372 strip px over 8 with a full-window scrim, 0 with this
    // chart-area one).
    let dim = fixed(
        "co-dim",
        140,
        Rect::new(window.x, window.y + chrome_h, window.width, window.height - chrome_h),
        &format!("background: {}", look.backdrop),
    );
    let shadow = fixed("co-shadow", 145, Rect::new(g.frame.x + 3.0, g.frame.y + 3.0, g.frame.width, g.frame.height), "background: rgba(0,0,0,0.4)");
    let bg = fixed("co-bg", 149, g.frame, &format!("background: {}", t.background));
    let mut root = El::new("div.k-root.m-root")
        .css("height: 100%")
        .child(dialog)
        .child(dim)
        .child(shadow)
        .child(bg)
        .children(super::frame_border(g.frame, &t.border))
        .children(super::hairline(g.frame.x, g.frame.y + 44.0, g.frame.right(), &t.header_border));
    if let Some(f) = g.footer {
        root = root.children(super::hairline(g.frame.x, f.y, g.frame.right(), &t.footer_border));
    }
    if let Some(tabs) = g.tabs {
        // +1 px tall: the tab bar's bottom hairline is stroked centred on
        // the slot's bottom edge, and slot paint clips to the slot box.
        // +1 px each side: the same hairline's round-cap end spill lands
        // outside the bar's own rect, in the frame's border columns.
        root = root.child(
            fixed("co-slot", 152, Rect::new(tabs.x - 1.0, tabs.y, tabs.width + 2.0, tabs.height + 1.0), "").slot(TABS),
        );
    }
    // +1 px wide/tall for the same edge-stroke reason (the body's own
    // scroll container clips its content; the slot must not clip the
    // container's own edge pixels).
    root = root.child(
        fixed("co-slot", 152, Rect::new(g.content.x, g.content.y, g.content.width + 1.0, g.content.height + 1.0), "").slot(BODY),
    );
    root
}

/// Paint one embed slot (`key` is the slot's [`BODY`]/[`TABS`] name): the
/// legacy renderer, in the app's own chart coordinate space (origin at the
/// chrome strip's bottom edge), at the shell geometry [`geometry`] settled
/// on. Hits go to a throwaway coordinator — the app's layout pass registered
/// the live zones. Takes the key as a plain `&str` so this crate needs no
/// uzor-framework dependency; the host's slot painter passes `f.key`.
pub fn paint_slot(
    ctx: &mut dyn uzor::render::RenderContext,
    key: &str,
    state: &CloudOverlayState,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    window: Rect,
    chrome_h: f64,
) {
    let g = geometry(&overlay_spec(state), window, chrome_h);
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
    let layer = LayerId::new("cloud-dom-body");
    let mut c = SlotChartCtx(ctx);
    c.save();
    c.translate(ox, oy);
    match key {
        TABS => {
            if let Some(tabs) = shell.tabs {
                draw_news_tabs(&mut c, &mut coord, &layer, tabs, state.news_tab.index(), &theme);
            }
        }
        BODY => {
            let mut result = CloudOverlayResult::default();
            draw_body_dom(&mut c, &mut coord, &layer, &shell, state, tt, &mut result);
        }
        _ => {}
    }
    c.restore();
}

/// What a DOM click means to the app: the legacy id it dispatches under.
/// The kit footer button is the shell's OK; the close button and everything
/// else already carry their legacy ids. Clicks outside the frame are NOT
/// DOM hits — they fall through to the app's dimmer strips.
pub fn app_id(clicked: &str) -> Option<String> {
    match clicked {
        "cloud_overlay:foot:0" => Some("cloud_overlay:ok".to_string()),
        r if r.starts_with("cloud_overlay:") => Some(r.to_string()),
        _ => None,
    }
}
