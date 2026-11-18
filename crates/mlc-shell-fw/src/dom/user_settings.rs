//! The user settings modal as uzor DOM — General (signed out, one profile) and
//! Persistence tabs.
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
use mlc_core::i18n::{current_language, Language, SettingsKey, UserSettingsKey};
use uzor::types::Rect;
use uzor::ui::dom::El;
use uzor::ui::domkit;

use super::sidebar_panels::{mid_text, top_text, At};

/// The dialog id (`us:close`, `us:foot:0`).
pub const ID: &str = "us";
/// The scrollbar's paint slot.
pub const SCROLLBAR: &str = "us:scrollbar";
/// Whole-body paint slot for tabs whose legacy renderer is still the paint source
/// (Server): DOM owns the shell/title/viewport, the legacy tab body paints here.
pub const BODY: &str = "us:body";
/// Whole-rail paint slot: legacy `draw_vertical_rail` paints the icon column;
/// the DOM keeps transparent per-cell buttons for clicks only.
pub const RAIL: &str = "us:rail";

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
div.u-body {{ position: absolute }}
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

pub(crate) fn tab_icon(tab: UserSettingsTab) -> &'static str {
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

    /// `draw_radio_group` in its compact form: 32 px rows 4 px apart. Each row
    /// carries a DOM hit `{id}:{key}` (G25 input) so the shell can forward picks;
    /// the ring/dot/label stay the pixel-exact stand-in until the catalog
    /// `domkit::radio_group` flex column is verified inside absolute wrappers.
    fn radios(&mut self, id: &str, opts: &[(&str, &str)], selected: usize, measure: &mut dyn FnMut(&str, &str) -> f64) -> f64 {
        let l = self.look;
        // Catalog radio group (G25). The id prefix is the legacy one so the row
        // hits (`user_settings:language:{code}` …) match the app-hosted modal for
        // hover + click forwarding; Look colours map onto the kit's `--k-*` vars.
        let prefix = match id {
            "us:lang" => "language",
            "us:shape" => "control_point_shape",
            "us:size" => "control_point_size",
            other => other,
        };
        // Pixel-exact stand-in with legacy hit ids + app-driven hover. The catalog
        // `radio_group` (pen-anchored, uzor >= 1.5.49) is used by new surfaces; this
        // parity-critical modal keeps the stand-in until baseline match is exact.
        let mut y = self.y;
        let mut last = y;
        for (i, (key, label)) in opts.iter().enumerate() {
            let on = i == selected;
            // Hover is pointer-driven via `.us-row:hover` (sheet rule), matching the
            // app-hosted modal's legacy-zone hover rather than app hover state.
            self.els.push(
                El::new("div.us-row")
                    .css(format!("left: 0px; top: {y}px; width: {ROWS_W}px; height: 32px; background: transparent"))
                    .id(format!("user_settings:{prefix}:{key}"))
                    .tab(),
            );
            let (cx, cy) = (11.0, y + 16.0);
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
    r.y = r.radios("us:lang", &langs, sel, measure) + 16.0;

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
    r.y = r.radios("us:shape", &shapes, shape, measure) + 12.0;
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
    r.y = r.radios("us:size", &sizes, size, measure) + 16.0;

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
        r.els.push(bx(x, r.y, seg_w, 28.0, "transparent").id(format!("user_settings:toast_anchor:{v:?}")).tab());
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
    r.els.push(bx(0.0, r.y, bw, 24.0, "transparent").id("user_settings:show_wizard").tab());
    r.els.push(bx(0.0, r.y, bw, 24.0, &format!("background: {}; border-radius: 4px", if hov { &look.wizard_hover } else { &look.hover })));
    r.els.push(ring(0.0, r.y, bw, 24.0, 4.0, &look.line));
    let c = if hov { look.wizard_text_hover.clone() } else { look.wizard_text.clone() };
    r.els.push(mid_text(UserSettingsKey::ShowWelcomeWizard.get(lang), bw / 2.0, r.y + 12.0, At::C, "11px sans-serif", &c, measure));
    r.y += 32.0;
    (r.els, r.y)
}

/// The Persistence tab: two checkbox rows with wrapped hints, exactly the
/// legacy `persist_checkbox_row` geometry (24 px row, 16 px box, 12 px label,
/// 11 px hint at +26, 14 px line height, +12 trailing). The hit rect spans the
/// row plus its hint and carries the legacy id so clicks forward unchanged.
/// Unlike General's radios there is no hover fill in the legacy paint, so the
/// hit div stays transparent with no `:hover` rule.
fn persistence_rows(s: &UserSettingsState, look: &Look, measure: &mut dyn FnMut(&str, &str) -> f64) -> (Vec<El>, f64) {
    let lang = current_language();
    let mut els = Vec::new();
    let mut y = 0.0;
    let w = ROWS_W;
    let text_w = (w - 26.0).max(0.0);
    let rows = [
        (s.persist_toolbar_last_tools, "persist_toolbar_last_tools", UserSettingsKey::PersistToolbarLastTools.get(lang), UserSettingsKey::PersistToolbarLastToolsHint.get(lang)),
        (s.persist_toolbar_toggles, "persist_toolbar_toggles", UserSettingsKey::PersistToolbarToggles.get(lang), UserSettingsKey::PersistToolbarTogglesHint.get(lang)),
    ];
    for (on, hit_id, label, hint) in rows {
        let hint_lines = wrap(&hint, text_w, "11px sans-serif", measure);
        let hint_h = hint_lines.len().max(1) as f64 * 14.0;
        let hit_h = 24.0 + 2.0 + hint_h;
        els.push(bx(0.0, y, w, hit_h, "transparent").id(format!("user_settings:{hit_id}")).tab());
        els.push(ring(0.0, y + 4.0, 16.0, 16.0, 2.0, &look.line));
        if on {
            els.push(bx(3.0, y + 7.0, 10.0, 10.0, &format!("background: {}; border-radius: 1px", look.text_on)));
        }
        let label = fit(&label, text_w, "12px sans-serif", measure);
        els.push(mid_text(&label, 26.0, y + 12.0, At::L, "12px sans-serif", &look.text, measure));
        let desc_y = y + 26.0;
        for (i, line) in hint_lines.iter().enumerate() {
            els.push(top_text(line, 26.0, desc_y + i as f64 * 14.0, At::L, "11px sans-serif", &look.muted, measure));
        }
        y = desc_y + hint_h + 12.0;
    }
    (els, y)
}

/// The Performance tab's two slider controls, painted by the legacy
/// `render_single_slider` into paint slots (the chart-as-SlotPainter pattern):
/// DOM owns the row layout, hits and scroll; the slider's label/track/handle/
/// number-input stay pixel-exact until catalog adoption replaces the slot.
pub const SLIDER_BG: &str = "us:slider:bg";
pub const SLIDER_MAX: &str = "us:slider:max";

/// The Performance tab: indicator-recalc radio group (full rows with
/// descriptions), the diagnostics checkbox, and the two data/cache sliders.
/// Geometry mirrors `render_performance_tab` + `draw_radio_group(compact=false)`
/// + `render_single_slider` row placement.
fn performance_rows(s: &UserSettingsState, look: &Look, measure: &mut dyn FnMut(&str, &str) -> f64) -> (Vec<El>, f64) {
    let lang = current_language();
    let mut els = Vec::new();
    let w = ROWS_W;
    let mut y = 0.0;
    let muted = look.muted.clone();

    els.push(top_text(UserSettingsKey::SectionIndicatorRecalc.get(lang), 0.0, y, At::L, "12px sans-serif", &muted, measure));
    y += 22.0;

    // Full-row radios: 52 px rows 8 px apart, ring centre at +14, label top +4,
    // description top +24, text at x+30. Hover fill via `.us-row:hover` (6 px).
    let options = [
        ("PerFrame", "Per Frame", "Recalculate once per rendered frame. Best balance of accuracy and performance."),
        ("PerTick", "Per Tick", "Recalculate on every incoming trade. Most accurate, higher CPU. Best for agents."),
        ("PerBar", "Per Bar", "Recalculate only when a new bar closes. Minimal CPU, suitable for long timeframes."),
    ];
    let selected = match s.recalc_mode_label.as_str() {
        "Per Tick" => 1,
        "Per Bar" => 2,
        _ => 0,
    };
    for (i, (key, label, desc)) in options.iter().enumerate() {
        let on = i == selected;
        els.push(
            El::new("div.us-row")
                .css(format!("left: 0px; top: {y}px; width: {w}px; height: 52px; background: transparent"))
                .id(format!("user_settings:recalc_mode:{key}"))
                .tab(),
        );
        let (cx, cy) = (11.0, y + 14.0);
        let ring_c = if on { &look.accent } else { &look.line };
        els.push(bx(cx - 7.75, cy - 7.75, 15.5, 15.5, &format!("border: 1.5px solid {ring_c}; border-radius: 7.75px")));
        if on {
            els.push(bx(cx - 4.0, cy - 4.0, 8.0, 8.0, &format!("background: {}; border-radius: 4px", look.accent)));
        }
        els.push(top_text(label, 30.0, y + 4.0, At::L, "13px sans-serif", if on { &look.text_on } else { &look.text }, measure));
        els.push(top_text(desc, 30.0, y + 24.0, At::L, "11px sans-serif", &muted, measure));
        y += 60.0;
    }
    y = y - 8.0 + 24.0;

    // Diagnostics checkbox row (24 px) at +18 under its section label.
    els.push(top_text(UserSettingsKey::SectionDiagnostics.get(lang), 0.0, y, At::L, "12px sans-serif", &muted, measure));
    let cb_y = y + 18.0;
    els.push(bx(0.0, cb_y, w, 24.0, "transparent").id("user_settings:diagnostics_toggle").tab());
    els.push(ring(0.0, cb_y + 4.0, 16.0, 16.0, 2.0, &look.line));
    if s.diagnostics_enabled {
        els.push(bx(3.0, cb_y + 7.0, 10.0, 10.0, &format!("background: {}; border-radius: 1px", look.text_on)));
    }
    els.push(mid_text(UserSettingsKey::EnableDiagnosticLogging.get(lang), 26.0, cb_y + 12.0, At::L, "12px sans-serif", &look.text, measure));
    els.push(top_text("Logs trade and recalculation counts every 5 seconds.", 26.0, cb_y + 26.0, At::L, "11px sans-serif", &muted, measure));
    y = cb_y + 26.0 + 15.0;

    // Data & cache: two slider rows (28 px) each followed by an 11 px description.
    y += 24.0;
    els.push(top_text(UserSettingsKey::SectionDataCache.get(lang), 0.0, y, At::L, "12px sans-serif", &muted, measure));
    y += 22.0;
    for (slot, desc_key) in [(SLIDER_BG, UserSettingsKey::DescBgBars), (SLIDER_MAX, UserSettingsKey::DescMaxBars)] {
        els.push(bx(0.0, y, w, 28.0, "transparent").id(format!("user_settings:{}", if slot == SLIDER_BG { "data_bg_bars" } else { "data_max_bars" })).tab());
        els.push(bx(0.0, y, w, 28.0, "").paint_slot(slot));
        y += 28.0;
        els.push(top_text(desc_key.get(lang), 4.0, y, At::L, "11px sans-serif", &muted, measure));
        y += 14.0;
        y += 8.0;
    }
    (els, y)
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
    let _ = hover;
    let tabs = s.visible_tabs();
    let active = s.active_tab;
    // The app highlights `position(active).unwrap_or(0)`: a tab the rail no longer
    // shows (e.g. Sync) falls back to the first cell, not to no highlight.
    let active_i = tabs.iter().position(|t| *t == active).unwrap_or(0);
    // Rail pixels come from legacy `draw_vertical_rail` in the RAIL slot (the
    // shell's own raster); the DOM keeps transparent per-cell buttons for hits.
    // Inline css beats the shared shell sheet rules for the stand-in look.
    let rail = El::new("div.m-rail")
        .css("border-right: 1px solid transparent; margin-left: -1px")
        .paint_slot(RAIL)
        .children(tabs.iter().map(|t| {
            El::new("button.m-tab")
                .id(format!("user_settings:tab:{}", t.id()))
                .tab()
                .css("background: transparent; border: none; padding: 0; width: 48px; height: 44px")
        }));
    let (rows, content_h) = match active {
        UserSettingsTab::Persistence => persistence_rows(s, look, measure),
        UserSettingsTab::Performance => performance_rows(s, look, measure),
        // The legacy Sync arm paints nothing (`=> {}`); the shell alone is the tab.
        UserSettingsTab::Sync => (Vec::new(), 0.0),
        // These bodies (incl. their scrollbars) paint through the BODY slot.
        UserSettingsTab::Server | UserSettingsTab::Vault | UserSettingsTab::Account | UserSettingsTab::Shares => (Vec::new(), 0.0),
        // General with the profile section (signed in / multi-profile) is slot-painted;
        // signed-out single-profile General is fully DOM below.
        UserSettingsTab::General if s.account_signed_in || s.multi_profile => (Vec::new(), 0.0),
        _ => general_rows(s, look, measure),
    };
    let scroll = match active {
        UserSettingsTab::Persistence => s.persistence_tab_scroll.animated_offset(),
        UserSettingsTab::Performance => s.performance_tab_scroll.animated_offset(),
        UserSettingsTab::Sync | UserSettingsTab::Server | UserSettingsTab::Vault | UserSettingsTab::Account | UserSettingsTab::Shares => 0.0,
        UserSettingsTab::General if s.account_signed_in || s.multi_profile => 0.0,
        _ => s.general_tab_scroll.animated_offset(),
    };
    let view = El::new("div.u-view").child(El::new("div.u-rows").css(format!("top: {}px; height: {content_h}px", -scroll)).children(rows));
    let title = top_text(active.label(), GAP + PAD, PAD, At::L, "bold 14px sans-serif", &look.text, measure);
    let mut content = El::new("div.u-content").child(title).child(view);
    let slot_body = matches!(active, UserSettingsTab::Server | UserSettingsTab::Vault | UserSettingsTab::Account | UserSettingsTab::Shares)
        || (active == UserSettingsTab::General && (s.account_signed_in || s.multi_profile));
    if slot_body {
        content = content.child(
            El::new("div.u-body").paint_slot(BODY).css(format!(
                "left: {}px; top: {}px; width: {VIEW_W}px; height: {VIEW_H}px",
                VIEW_X - RAIL_W,
                VIEW_Y - HEADER_H
            )),
        );
    }
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
    // The old modal shell captions this dialog with the shared settings title
    // ("Settings"), not the longer user-settings key; match it pixel-for-pixel.
    let title_s = SettingsKey::Title.get(current_language());
    let dialog = El::new("div.k-modal").child(El::new("div.k-scrim")).child(
        El::new("div.k-dialog")
            .css(format!("top: {}px; left: {}px; width: {}px; height: {}px", at.y, at.x, at.width, at.height))
            .child(
                El::new("div.k-dialog-head")
                    .child(El::new("div.k-dialog-title").text(title_s))
                    .child(El::new(if hover.close { "button.k-act.hov" } else { "button.k-act" }).id("user_settings:close").tab().svg(domkit::icon(domkit::icons::CLOSE))),
            )
            .child(El::new("div.k-dialog-body").child(body))
            .child(El::new("div.k-dialog-foot").child(El::new(if hover.ok { "button.k-btn.primary.hov" } else { "button.k-btn.primary" }).id("user_settings:ok").tab().text("OK"))),
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
    let tab_i = active_i as f64;
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
    let off = match s.active_tab {
        UserSettingsTab::Persistence => s.persistence_tab_scroll.animated_offset(),
        UserSettingsTab::Performance => s.performance_tab_scroll.animated_offset(),
        UserSettingsTab::Sync => 0.0,
        _ => s.general_tab_scroll.animated_offset(),
    };
    (content_h, VIEW_H, off)
}
