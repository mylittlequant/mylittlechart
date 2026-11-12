//! The user settings modal as uzor DOM — General tab, signed out, one profile.
//!
//! Was: `mlc-app-layout` `layout/modals/user_settings.rs`
//! (`render_user_settings_modal` + `render_general_tab`), hand-drawn through
//! `begin_modal_shell`. Now its paint is this tree: the shell is the same
//! `domkit::dialog` as the chart settings modal (tab rail, header, OK footer), the
//! tab body is DOM boxes and text at the app's geometry.
//!
//! In place: the app still lays the modal out, registers its hit zones and runs its
//! input (hover, clicks, the wheel, the scrollbar drag) — its paint goes nowhere
//! (`ChartPanelApp::user_settings_on_dom`, `PaintOff`). This tree reads the app's
//! state (the active tab, the choices, the hovered item, the scroll offset) and
//! paints it; it takes no input of its own. Any other state (another tab, signed
//! in, several profiles) is drawn by the app as before.
//!
//! The radio rows are a local stand-in for a uzor catalog radio (`domkit` has
//! none in 1.5.41; see uzor-gaps G25): a ring and a dot as CSS boxes.

use mlc_app_layout::ui::modal_settings::{UserSettingsState, UserSettingsTab};
use mlc_core::drawing::primitives_v2::{ControlPointShape, ControlPointSize};
use mlc_core::i18n::{current_language, Language, UserSettingsKey};
use uzor::types::Rect;
use uzor::ui::dom::El;
use uzor::ui::domkit;

use super::sidebar_panels::{mid_text, top_text, At};

/// The dialog id (`us:close`, `us:foot:0`).
pub const ID: &str = "us";
/// The scrollbar's paint slot.
pub const SCROLLBAR: &str = "us:scrollbar";

pub const W: f64 = 540.0;
pub const H: f64 = 500.0;
const HEADER_H: f64 = 44.0;
const FOOTER_H: f64 = 52.0;
const RAIL_W: f64 = 48.0;
/// The body's inset (the app's `padding`).
const PAD: f64 = 20.0;
/// The gap between the rail and the tab content (the app's `sidebar_gap`).
const GAP: f64 = 6.0;
const SCROLLBAR_W: f64 = 8.0;
const SCROLLBAR_PAD: f64 = 4.0;
/// The scroll viewport, relative to the frame: below the tab title.
pub const VIEW_X: f64 = RAIL_W + GAP + PAD;
pub const VIEW_Y: f64 = HEADER_H + PAD + 30.0;
pub const VIEW_W: f64 = W - RAIL_W - GAP - PAD * 2.0;
pub const VIEW_H: f64 = H - HEADER_H - FOOTER_H - PAD - 30.0;
/// The rows' width (the viewport less the scrollbar).
const ROWS_W: f64 = VIEW_W - SCROLLBAR_W;

/// Theme colours, as the old modal resolved them.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub shell: super::chart_settings::Look,
    pub heading: String,
    pub text: String,
    pub text_on: String,
    pub muted: String,
    pub line: String,
    pub hover: String,
    pub active: String,
    pub accent: String,
    pub control_bg: String,
    pub wizard_hover: String,
    pub wizard_text: String,
    pub wizard_text_hover: String,
}

impl Look {
    pub fn of(app: &mlc_app::ChartApp) -> Self {
        let tt = app.panel_app.toolbar_theme_for_render();
        Look {
            shell: super::chart_settings::Look::of(app),
            heading: mlc_core::apply_opacity(&tt.accent, 0.85),
            text: tt.item_text.clone(),
            text_on: tt.item_text_active.clone(),
            muted: tt.item_text_muted.clone(),
            line: tt.separator.clone(),
            hover: tt.item_bg_hover.clone(),
            active: tt.item_bg_active.clone(),
            accent: tt.accent.clone(),
            control_bg: tt.dropdown_bg.clone(),
            wizard_hover: mlc_core::apply_opacity(&tt.item_text, 0.10),
            wizard_text: mlc_core::apply_opacity(&tt.item_text, 0.55),
            wizard_text_hover: mlc_core::apply_opacity(&tt.item_text, 0.80),
        }
    }

    /// The chart settings shell's rules, then this body's.
    pub fn css(&self) -> String {
        format!(
            "{}
div.u-content {{ flex-grow: 1; position: relative }}
div.u-view {{ position: absolute; left: {vl}px; top: {vt}px; width: {ROWS_W}px; height: {VIEW_H}px; overflow: hidden }}
div.u-rows {{ position: absolute; left: 0px; width: {ROWS_W}px }}
div.u-scroll {{ position: absolute }}
div.s-at {{ position: absolute; white-space: nowrap; height: 40px }}
div.s-fill {{ position: absolute }}
div.u-box {{ position: absolute }}
button.m-tab.hov {{ background: {hover}; color: {text} }}
div.k-dialog-head > button.k-act.hov {{ color: {text_on} }}
div.k-dialog-foot > button.k-btn.primary.hov {{ background: {accent_hover} }}
div.u-rule {{ position: fixed; z-index: 152; background: {half} }}
div.u-under {{ position: fixed; z-index: 146; background: {bg} }}
div.u-tab-edge {{ position: fixed; z-index: 151; background: {on} }}
div.u-tab-edge.hov {{ background: {hover} }}",
            self.shell.css(),
            vl = VIEW_X - RAIL_W,
            vt = VIEW_Y - HEADER_H,
            bg = self.shell.bg,
            hover = self.hover,
            text = self.text,
            text_on = self.text_on,
            accent_hover = self.shell.accent_hover,
            on = self.shell.accent,
            half = {
                let (r, g, b, a) = mlc_core::parse_css_color(&self.line);
                format!("rgba({r},{g},{b},{:.3})", a as f64 / 255.0 * 0.5)
            },
        )
    }
}

/// Where the frame goes: centred on the app's screen (below the chrome strip) or
/// where the user moved it, clamped on screen; one pixel larger all round for the
/// half-alpha border (as the chart settings frame).
pub fn frame(window: Rect, chrome_h: f64, state: &UserSettingsState) -> Rect {
    let (sw, sh) = (window.width, window.height - chrome_h);
    let (x, y) = state.position.unwrap_or(((sw - W) / 2.0, (sh - H) / 2.0));
    let clamp = |p: f64, size: f64, screen: f64| if size >= screen { 0.0 } else { p.clamp(0.0, screen - size) };
    Rect::new(window.x + clamp(x, W, sw) - 1.0, window.y + chrome_h + clamp(y, H, sh) - 1.0, W + 2.0, H + 2.0)
}

fn tab_icon(tab: UserSettingsTab) -> &'static str {
    use mlc_app_layout::ui::Icon;
    match tab {
        UserSettingsTab::General => Icon::Settings.svg(),
        UserSettingsTab::Sync | UserSettingsTab::Vault => Icon::Lock.svg(),
        UserSettingsTab::Persistence | UserSettingsTab::Shares => Icon::Pin.svg(),
        UserSettingsTab::Performance => Icon::Grid.svg(),
        UserSettingsTab::Server => Icon::Layers.svg(),
        UserSettingsTab::Account | UserSettingsTab::Friends => Icon::User.svg(),
        UserSettingsTab::Admin => Icon::Shield.svg(),
    }
}

/// A box at `l, t` (`w`×`h`) with extra CSS.
fn bx(l: f64, t: f64, w: f64, h: f64, css: &str) -> El {
    El::new("div.u-box").css(format!("left: {l}px; top: {t}px; width: {w}px; height: {h}px; {css}"))
}

/// The app's `stroke_rounded_rect(x, y, w, h, r)` at 1 px: a border box centred
/// on the path.
fn ring(x: f64, y: f64, w: f64, h: f64, r: f64, color: &str) -> El {
    bx(x - 0.5, y - 0.5, w + 1.0, h + 1.0, &format!("border: 1px solid {color}; border-radius: {}px", r + 0.5))
}

/// The app's `fit_text_to_width`.
fn fit(text: &str, max_w: f64, font: &str, measure: &mut dyn FnMut(&str, &str) -> f64) -> String {
    if max_w <= 0.0 {
        return String::new();
    }
    if measure(font, text) <= max_w {
        return text.to_string();
    }
    let ell = "\u{2026}";
    let avail = max_w - measure(font, ell);
    if avail <= 0.0 {
        return String::new();
    }
    let mut s = text.to_string();
    while !s.is_empty() && measure(font, &s) > avail {
        s.pop();
    }
    s.push_str(ell);
    s
}

/// The app's `wrap_to_width` (`user_settings.rs`).
fn wrap(text: &str, max_w: f64, font: &str, measure: &mut dyn FnMut(&str, &str) -> f64) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut rest = para.trim();
        if rest.is_empty() {
            out.push(String::new());
            continue;
        }
        while !rest.is_empty() {
            if measure(font, rest) <= max_w {
                out.push(rest.to_string());
                break;
            }
            let (mut fit_end, mut fit_space) = (0usize, 0usize);
            let mut acc = String::new();
            for (i, ch) in rest.char_indices() {
                acc.push(ch);
                if measure(font, &acc) > max_w {
                    break;
                }
                fit_end = i + ch.len_utf8();
                if ch == ' ' {
                    fit_space = fit_end;
                }
            }
            if fit_end == 0 {
                let ch = rest.chars().next().unwrap();
                out.push(ch.to_string());
                rest = &rest[ch.len_utf8()..];
                continue;
            }
            let cut = if fit_space > 0 { fit_space } else { fit_end };
            out.push(rest[..cut].trim_end().to_string());
            rest = rest[cut..].trim_start();
        }
    }
    out
}

/// The General tab's rows, in content coordinates (0 = the top of the scrolled
/// content), and the content height.
struct Rows<'a> {
    els: Vec<El>,
    y: f64,
    look: &'a Look,
    hovered: Option<&'a str>,
}

impl Rows<'_> {
    fn heading(&mut self, text: &str, measure: &mut dyn FnMut(&str, &str) -> f64) {
        self.els.push(top_text(text, 0.0, self.y, At::L, "600 11px sans-serif", &self.look.heading, measure));
    }

    /// `draw_radio_group` in its compact form: 32 px rows 4 px apart.
    fn radios(&mut self, opts: &[(&str, &str)], selected: usize, measure: &mut dyn FnMut(&str, &str) -> f64) -> f64 {
        let l = self.look;
        let mut y = self.y;
        let mut last = y;
        for (i, (key, label)) in opts.iter().enumerate() {
            let on = i == selected;
            if self.hovered == Some(*key) {
                self.els.push(bx(0.0, y, ROWS_W, 32.0, &format!("background: {}; border-radius: 6px", l.hover)));
            }
            let (cx, cy) = (11.0, y + 16.0);
            // The 1.5 px ring of radius 7, centred on its path.
            let ring_c = if on { &l.accent } else { &l.line };
            self.els.push(bx(cx - 7.75, cy - 7.75, 15.5, 15.5, &format!("border: 1.5px solid {ring_c}; border-radius: 7.75px")));
            if on {
                self.els.push(bx(cx - 4.0, cy - 4.0, 8.0, 8.0, &format!("background: {}; border-radius: 4px", l.accent)));
            }
            self.els.push(mid_text(label, 30.0, cy, At::L, "13px sans-serif", if on { &l.text_on } else { &l.text }, measure));
            last = y;
            y += 36.0;
        }
        last + 32.0
    }

    /// The bare checkbox rows: a 16 px box (2 px corners), a 12 px label.
    fn check(&mut self, on: bool, label: &str, measure: &mut dyn FnMut(&str, &str) -> f64) {
        let l = self.look;
        let y = self.y;
        self.els.push(ring(0.0, y + 4.0, 16.0, 16.0, 2.0, &l.line));
        if on {
            self.els.push(bx(3.0, y + 7.0, 10.0, 10.0, &format!("background: {}; border-radius: 1px", l.text_on)));
        }
        self.els.push(mid_text(label, 26.0, y + 12.0, At::L, "12px sans-serif", &l.text, measure));
    }
}

fn general_rows(s: &UserSettingsState, look: &Look, measure: &mut dyn FnMut(&str, &str) -> f64) -> (Vec<El>, f64) {
    let lang = current_language();
    let mut r = Rows { els: Vec::new(), y: 0.0, look, hovered: s.hovered_item_id.as_deref() };
    let w = ROWS_W;

    r.heading(UserSettingsKey::SectionLanguage.get(lang), measure);
    r.y += 20.0;
    let langs: Vec<(&str, &str)> = Language::all().iter().map(|l| (l.code(), l.native_name())).collect();
    let sel = Language::all().iter().position(|l| l.code() == s.language.as_str()).unwrap_or(0);
    r.y = r.radios(&langs, sel, measure) + 16.0;

    r.heading(UserSettingsKey::SectionControlPoints.get(lang), measure);
    r.y += 20.0;
    let muted = look.muted.clone();
    r.els.push(top_text(UserSettingsKey::ControlPointShapeLabel.get(lang), 0.0, r.y, At::L, "11px sans-serif", &muted, measure));
    r.y += 16.0;
    let shapes = [
        ("Circle", UserSettingsKey::ControlPointShapeCircle.get(lang)),
        ("RoundedSquare", UserSettingsKey::ControlPointShapeRoundedSquare.get(lang)),
        ("Square", UserSettingsKey::ControlPointShapeSquare.get(lang)),
    ];
    let shape = match s.control_point_shape {
        ControlPointShape::Circle => 0,
        ControlPointShape::RoundedSquare => 1,
        ControlPointShape::Square => 2,
    };
    r.y = r.radios(&shapes, shape, measure) + 12.0;
    r.els.push(top_text(UserSettingsKey::ControlPointSizeLabel.get(lang), 0.0, r.y, At::L, "11px sans-serif", &muted, measure));
    r.y += 16.0;
    let sizes = [
        ("Small", UserSettingsKey::ControlPointSizeSmall.get(lang)),
        ("Medium", UserSettingsKey::ControlPointSizeMedium.get(lang)),
        ("Large", UserSettingsKey::ControlPointSizeLarge.get(lang)),
    ];
    let size = match s.control_point_size {
        ControlPointSize::Small => 0,
        ControlPointSize::Medium => 1,
        ControlPointSize::Large => 2,
    };
    r.y = r.radios(&sizes, size, measure) + 16.0;

    // The trading tab's topbar takeover: a checkbox row and its wrapped hint.
    r.check(s.trading_panel_topbar_takeover, UserSettingsKey::TradingPanelTopbarTakeover.get(lang), measure);
    let desc_y = r.y + 26.0;
    let hint = wrap("Symbol/exchange controls move to the topbar while the Trading tab is focused.", (w - 26.0).max(0.0), "11px sans-serif", measure);
    for (i, line) in hint.iter().enumerate() {
        r.els.push(top_text(line, 26.0, desc_y + i as f64 * 14.0, At::L, "11px sans-serif", &muted, measure));
    }
    r.y = desc_y + hint.len().max(1) as f64 * 14.0 + 16.0;

    r.heading("NOTIFICATIONS", measure);
    r.y += 20.0;
    let label = fit("Show toasts (events always go to the panel)", (w - 26.0).max(0.0), "12px sans-serif", measure);
    r.check(s.toasts_enabled, &label, measure);
    r.y += 36.0;

    // The toast corner: four segments.
    let anchors = [
        (mlc_core::ToastAnchor::TopLeft, "Top-Left"),
        (mlc_core::ToastAnchor::TopRight, "Top-Right"),
        (mlc_core::ToastAnchor::BottomLeft, "Bottom-Left"),
        (mlc_core::ToastAnchor::BottomRight, "Bottom-Right"),
    ];
    let seg_w = ((w - 18.0) / 4.0).max(1.0);
    for (i, (v, label)) in anchors.iter().enumerate() {
        let x = i as f64 * (seg_w + 6.0);
        let on = s.toast_anchor == *v;
        let hov = r.hovered == Some(format!("toast_anchor:{v:?}").as_str());
        let bg = if on { &look.active } else if hov { &look.hover } else { &look.control_bg };
        r.els.push(bx(x, r.y, seg_w, 28.0, &format!("background: {bg}; border-radius: 4px")));
        r.els.push(ring(x, r.y, seg_w, 28.0, 4.0, &look.line));
        let t = fit(label, (seg_w - 8.0).max(1.0), "11px sans-serif", measure);
        r.els.push(mid_text(&t, x + seg_w / 2.0, r.y + 14.0, At::C, "11px sans-serif", if on { &look.text_on } else { &look.text }, measure));
    }
    r.y += 44.0;

    r.heading(UserSettingsKey::SectionVersion.get(lang), measure);
    r.y += 18.0;
    let text = look.text.clone();
    r.els.push(top_text(&format!("v{}", mlc_app_layout::layout::modals::user_settings::VERSION), 0.0, r.y, At::L, "700 18px sans-serif", &text, measure));
    r.y += 30.0;

    let bw = w.min(180.0);
    let hov = r.hovered == Some("show_wizard");
    r.els.push(bx(0.0, r.y, bw, 24.0, &format!("background: {}; border-radius: 4px", if hov { &look.wizard_hover } else { &look.hover })));
    r.els.push(ring(0.0, r.y, bw, 24.0, 4.0, &look.line));
    let c = if hov { look.wizard_text_hover.clone() } else { look.wizard_text.clone() };
    r.els.push(mid_text(UserSettingsKey::ShowWelcomeWizard.get(lang), bw / 2.0, r.y + 12.0, At::C, "11px sans-serif", &c, measure));
    r.y += 32.0;
    (r.els, r.y)
}

/// What the app's input says the pointer is on (its own hit test, last frame).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hover {
    pub close: bool,
    pub ok: bool,
    /// A rail tab, by index among the visible tabs.
    pub tab: Option<usize>,
}

impl Hover {
    /// From the app's coordinator: `user_settings:close`, `:ok`, `:tab:{id}`.
    pub fn of(coord: &uzor::input::InputCoordinator, tabs: &[UserSettingsTab]) -> Self {
        let on = |id: &str| coord.is_hovered(&uzor::WidgetId::from(id));
        Hover {
            close: on("user_settings:close"),
            ok: on("user_settings:ok"),
            tab: tabs.iter().position(|t| on(&format!("user_settings:tab:{}", t.id()))),
        }
    }
}

/// The whole tree at frame `at`, and the content height (for the scrollbar slot).
///
/// No element carries an id: nothing here is a hit, so every press, move and wheel
/// over the modal reaches the app as unhandled input, as it did.
pub fn tree(s: &UserSettingsState, look: &Look, hover: Hover, at: Rect, measure: &mut dyn FnMut(&str, &str) -> f64) -> (El, f64) {
    let tabs = s.visible_tabs();
    let active = s.active_tab;
    let rail = El::new("div.m-rail").children(tabs.iter().enumerate().map(|(i, t)| {
        let cls = if *t == active { "button.m-tab.on" } else if hover.tab == Some(i) { "button.m-tab.hov" } else { "button.m-tab" };
        El::new(cls).svg(tab_icon(*t))
    }));
    let (rows, content_h) = general_rows(s, look, measure);
    let scroll = s.general_tab_scroll.animated_offset();
    let view = El::new("div.u-view").child(El::new("div.u-rows").css(format!("top: {}px; height: {content_h}px", -scroll)).children(rows));
    let title = top_text(active.label(), GAP + PAD, PAD, At::L, "bold 14px sans-serif", &look.text, measure);
    let mut content = El::new("div.u-content").child(title).child(view);
    if content_h > VIEW_H {
        let eff = VIEW_H.min(content_h);
        content = content.child(
            El::new("div.u-scroll").paint_slot(SCROLLBAR).css(format!(
                "left: {}px; top: {}px; width: {SCROLLBAR_W}px; height: {}px",
                VIEW_X + VIEW_W - SCROLLBAR_W - RAIL_W,
                VIEW_Y - HEADER_H + SCROLLBAR_PAD,
                eff - SCROLLBAR_PAD * 2.0
            )),
        );
    }
    let body = El::new("div.m-body-row").css("display: flex; flex-direction: row; flex-grow: 1").child(rail).child(content);
    // `domkit::dialog`'s shape, without its ids (they would be hits).
    let title_s = UserSettingsKey::Title.get(current_language());
    let dialog = El::new("div.k-modal").child(El::new("div.k-scrim")).child(
        El::new("div.k-dialog")
            .css(format!("top: {}px; left: {}px; width: {}px; height: {}px", at.y, at.x, at.width, at.height))
            .child(
                El::new("div.k-dialog-head")
                    .child(El::new("div.k-dialog-title").text(title_s))
                    .child(El::new(if hover.close { "button.k-act.hov" } else { "button.k-act" }).svg(domkit::icon(domkit::icons::CLOSE))),
            )
            .child(El::new("div.k-dialog-body").child(body))
            .child(El::new("div.k-dialog-foot").child(El::new(if hover.ok { "button.k-btn.primary.hov" } else { "button.k-btn.primary" }).text("OK"))),
    );
    // Two more rules the old modal drew along the tab rail: across the top of the
    // body, from the rail's edge, and down the rail's right side, one pixel inside
    // the shell's own.
    let (fx, fy) = (at.x + 1.0, at.y + 1.0);
    let rule = |l: f64, t: f64, w: f64, h: f64| El::new("div.u-rule").css(format!("left: {l}px; top: {t}px; width: {w}px; height: {h}px"));
    // The old frame filled its background under its own 2 px border's inner half;
    // the DOM dialog's border sits over whatever is behind it.
    let under = El::new("div.u-under").css(format!("left: {fx}px; top: {fy}px; width: {W}px; height: {H}px"));
    // The old active tab reached under the rail's edge rule (one more column).
    let tab_i = tabs.iter().position(|t| *t == active).unwrap_or(0) as f64;
    let tab_edge = El::new("div.u-tab-edge").css(format!("left: {}px; top: {}px; width: 1px; height: 44px", fx + RAIL_W - 1.0, fy + HEADER_H + tab_i * 44.0));
    let rules = [rule(fx + RAIL_W - 1.0, fy + HEADER_H + tab_i * 44.0, 1.0, 44.0), rule(fx + RAIL_W - 1.0, fy + HEADER_H, W - RAIL_W + 1.0, 1.0), rule(fx + RAIL_W, fy + HEADER_H, 1.0, H - HEADER_H - FOOTER_H)];
    // A hovered tab's fill reaches under the rail rule too.
    let hover_edge = hover.tab.filter(|&i| tabs.get(i) != Some(&active)).map(|i| {
        let y = fy + HEADER_H + i as f64 * 44.0;
        [El::new("div.u-tab-edge.hov").css(format!("left: {}px; top: {}px; width: 1px; height: 44px", fx + RAIL_W - 1.0, y)), rule(fx + RAIL_W - 1.0, y, 1.0, 44.0)]
    });
    let shadow = El::new("div.m-shadow").css(format!("top: {}px; left: {}px; width: {}px; height: {}px", at.y + 4.0, at.x + 4.0, W, H));
    (El::new("div.k-root.m-root").css("height: 100%").child(shadow).child(under).child(dialog).child(tab_edge).children(rules).children(hover_edge.into_iter().flatten()), content_h)
}

/// The scrollbar's numbers for the slot painter: content height, viewport height,
/// offset.
pub fn scrollbar(s: &UserSettingsState, content_h: f64) -> (f64, f64, f64) {
    (content_h, VIEW_H, s.general_tab_scroll.animated_offset())
}
