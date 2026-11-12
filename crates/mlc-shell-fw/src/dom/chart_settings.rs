//! The chart settings modal as uzor DOM — Instrument tab, candle-family charts.
//!
//! Was: `mlc-app-layout` `layout/modals/chart_settings.rs` (`render_settings_modal`
//! + `render_instrument_settings`), hand-drawn through `begin_modal_shell` with hit
//! rects on the app's own coordinator. Now: `domkit::dialog` with the tab rail, kit
//! checkboxes and DOM rows. Every click goes to the app's existing handler under the
//! id it had before (`ChartApp::chart_settings_click`), so the behaviour is the app's.
//!
//! Covered: the states `ChartPanelApp::chart_settings_on_dom` accepts. Any other
//! (another tab, another chart type, an open dropdown or colour picker, the
//! template menu, a resized frame) is still drawn by the app, frame for frame, so
//! the switch is invisible as long as both paint the same pixels.
//!
//! Scrolling: the body's offset is the app's own `ChartSettingsState::scroll`
//! (the wheel reaches it through [`HostedApp`](crate::HostedApp)), applied as a
//! negative top margin inside an `overflow: hidden` viewport, and the scrollbar is
//! the app's `draw_scrollbar` in a paint slot. Both go when framework-owned DOM
//! scroll (uzor `feat/dom-scroll`) is released.

use mlc_app_layout::layout::modals::chart_settings::InstrumentSettings;
use mlc_app_layout::ui::modal_settings::{ChartSettingsState, ChartSettingsTab};
use mlc_core::i18n::{current_language, t_settings, ClockKey, SettingsKey};
use uzor::types::Rect;
use uzor::ui::dom::{Control, El};
use uzor::ui::domkit;

/// The dialog id: `cs:close`, `cs:scrim`, `cs:foot:0` (OK), `cs:foot:1` (Template).
pub const ID: &str = "cs";
/// The scrollbar's paint slot.
pub const SCROLLBAR: &str = "cs:scrollbar";
/// The body viewport (its rect takes the wheel).
pub const VIEWPORT: &str = "cs:viewport";

pub const W: f64 = 620.0;
pub const H: f64 = 580.0;
const HEADER_H: f64 = 44.0;
const FOOTER_H: f64 = 52.0;
const RAIL_W: f64 = 48.0;
const PAD: f64 = 12.0;
const ROW: f64 = 32.0;
/// Body viewport: content minus the title band and the padding (old geometry).
pub const VIEW_H: f64 = H - HEADER_H - FOOTER_H - 30.0 - PAD;
const VIEW_W: f64 = W - RAIL_W - PAD * 2.0;
const SCROLLBAR_W: f64 = 8.0;
const CONTENT_W: f64 = VIEW_W - SCROLLBAR_W;
/// The split dropdowns: from 140 px in to 6 px short of the scrollbar.
const DD_W: f64 = CONTENT_W - 140.0 - 6.0;

/// Theme colours, as the old modal resolved them.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub bg: String,
    pub line: String,
    pub text: String,
    pub muted: String,
    pub on: String,
    pub on_text: String,
    pub control_bg: String,
    pub accent: String,
    pub accent_hover: String,
}

impl Look {
    pub fn of(app: &mlc_app::ChartApp) -> Self {
        let ft = app.panel_app.frame_theme_for_render();
        let tt = app.panel_app.toolbar_theme_for_render();
        Look {
            bg: ft.modal_bg.clone(),
            line: tt.separator.clone(),
            text: tt.item_text.clone(),
            muted: tt.item_text_muted.clone(),
            on: tt.item_bg_active.clone(),
            on_text: tt.item_text_active.clone(),
            control_bg: if mlc_app_layout::ui::overlay_fill::color_is_translucent(&tt.dropdown_bg) { tt.dropdown_bg.clone() } else { tt.background.clone() },
            accent: tt.accent.clone(),
            accent_hover: tt.accent_hover.clone(),
        }
    }

    pub fn css(&self) -> String {
        let Look { bg, line: _, text, muted, on, on_text, control_bg, accent, accent_hover } = self;
        let ddr_w = DD_W + 2.0;
        let (r, g, b, a) = mlc_core::parse_css_color(&self.line);
        let half = format!("rgba({r},{g},{b},{:.3})", a as f64 / 255.0 * 0.5);
        format!(
            "div.m-root {{ font-family: sans-serif }}
div.k-scrim {{ background: transparent }}
div.k-dialog {{ border-radius: 0; background: transparent; border: 2px solid {half}; box-shadow: none }}
div.k-dialog-head, div.k-dialog-body, div.k-dialog-foot {{ background: {bg} }}
div.m-shadow {{ position: fixed; z-index: 145; background: rgba(0,0,0,0.4) }}
div.k-dialog-head {{ height: 43px; flex-shrink: 0; padding: 0 6px 0 15px; font-size: 13px; font-weight: 400; color: {text}; border-bottom: 1px solid {half} }}
div.k-dialog-head > div.k-dialog-title {{ padding-top: 2px }}
div.k-dialog-head > button.k-act {{ color: {text} }}
div.k-dialog-head > button.k-act:hover {{ background: transparent }}
div.k-dialog-body {{ min-height: 0; flex-shrink: 1; padding: 0; gap: 0; flex-direction: row; align-items: stretch; justify-content: flex-start; color: {text} }}
div.k-dialog-foot {{ height: 52px; flex-shrink: 0; padding: 0 15px; align-items: flex-start; border-top: 2px solid {half}; position: relative }}
div.k-dialog-foot > button.k-btn {{ position: absolute; left: 14px; top: 8px; width: 82px; height: 34px; padding: 2px 0 0 0; border-radius: 0; font-size: 13px; font-weight: 400;
  background: transparent; border: 2px solid {half}; color: {text}; text-align: center }}
div.k-dialog-foot > button.k-btn.primary {{ position: static; margin-top: 9px; width: 70px; height: 32px; padding: 2px 0 0 0; border: none; background: {accent}; color: {on_text} }}
div.k-dialog-foot > button.k-btn.primary:hover {{ background: {accent_hover} }}
div.m-rail {{ width: 48px; flex-shrink: 0; display: flex; flex-direction: column; border-right: 1px solid {half}; margin-left: -1px }}
button.m-tab {{ width: 47px; height: 44px; padding: 13px 14px 13px 15px; color: {muted} }}
button.m-tab.on {{ background: {on}; border-left: 3px solid {accent}; padding-left: 12px; color: {on_text} }}
div.m-content {{ flex-grow: 1; padding: 12px 12px 0 12px; display: flex; flex-direction: column }}
div.m-title {{ height: 16px; flex-shrink: 0; margin: -3px 0 17px 0; font-size: 14px; font-weight: 700 }}
div.m-view {{ flex-shrink: 0; width: {VIEW_W}px; height: {VIEW_H}px; overflow: hidden; position: relative }}
div.m-rows {{ width: {CONTENT_W}px; display: flex; flex-direction: column }}
div.m-sec {{ height: 32px; padding-top: 2px; display: flex; align-items: center; font-size: 11px; color: {muted} }}
div.m-row {{ height: 32px; display: flex; align-items: center; font-size: 12px; color: {text} }}
div.k-check {{ gap: 11px; font-size: 12px; min-height: 0 }}
div.m-row > div.k-check, div.m-label > div.k-check {{ flex-grow: 1 }}
div.k-check > span {{ margin-top: 4px }}
span.k-box {{ width: 18px; height: 18px; padding: 0; margin: 0 0 0 -1px; border-radius: 4px; border: 2px solid {half}; background: {control_bg}; --check: {on_text}; --check-width: 2px }}
span.k-box:checked {{ background: {on}; border-color: {half} }}
div.k-check > span.k-box {{ margin: 0 0 0 -1px }}
div.m-label {{ width: 140px; flex-shrink: 0 }}
div.m-label.in {{ padding-left: 28px }}
div.m-label.t {{ padding-top: 4px }}
div.m-sw {{ width: 24px; height: 24px; border-radius: 4px; margin-right: 8px; position: relative }}
div.m-ring {{ position: absolute; top: -1px; left: -1px; width: 26px; height: 26px; border-radius: 5px; border: 2px solid {half} }}
div.m-dd > div.m-ring {{ width: {ddr_w}px; height: 30px }}
div.m-dd {{ width: {DD_W}px; height: 28px; display: flex; border-radius: 4px; background: {control_bg}; position: relative }}
button.m-dd-text {{ flex-grow: 1; height: 28px; padding: 3px 8px 0 8px; text-align: left; font-size: 12px; color: {text}; overflow: hidden }}
button.m-dd-chev {{ width: 21px; height: 28px; margin-left: -1px; border-left: 2px solid {half}; padding: 11px 7px 11px 6px; color: {text} }}
div.m-scroll {{ position: absolute; top: 0; right: 0; width: {SCROLLBAR_W}px; height: {VIEW_H}px }}
div.m-grip {{ position: fixed; z-index: 151; width: 10px; height: 10px; color: {muted} }}"
        )
    }
}

/// Where the frame goes: the old placement (centred in the chart content area) or
/// where the user moved it; one pixel larger all round for the half-alpha border.
pub fn frame(window: Rect, chrome_h: f64, origin: (f64, f64), state: &ChartSettingsState) -> Rect {
    let (sw, sh) = (window.width, window.height - chrome_h);
    let (x, y) = state.position.unwrap_or((origin.0 + (sw - origin.0 - W) / 2.0, origin.1 + (sh - origin.1 - H) / 2.0));
    let clamp = |p: f64, size: f64, screen: f64| if size >= screen { 0.0 } else { p.clamp(0.0, screen - size) };
    Rect::new(window.x + clamp(x, W, sw) - 1.0, window.y + chrome_h + clamp(y, H, sh) - 1.0, W + 2.0, H + 2.0)
}

fn item(id: &str) -> String {
    format!("{ID}:item:{id}")
}

fn section(text: &str) -> El {
    El::new("div.m-sec").text(text)
}

fn check(id: &str, on: bool, label: &str) -> El {
    El::new("div.m-row").child(domkit::checkbox(&item(id), on, label))
}

fn swatch(id: &str, color: &str) -> El {
    El::new("div.m-sw").id(item(id)).tab().name(id).css(format!("background: {color}")).child(El::new("div.m-ring"))
}

fn check_swatches(id: &str, on: bool, label: &str, up: (&str, &str), down: (&str, &str)) -> El {
    El::new("div.m-row")
        .child(El::new("div.m-label").child(domkit::checkbox(&item(id), on, label)))
        .child(swatch(up.0, up.1))
        .child(swatch(down.0, down.1))
}

/// The old split dropdown: the text half cycles, the chevron opens the list.
fn dropdown(key: &str, label: &str, value: &str, indent: bool) -> El {
    let chev = r#"<svg viewBox="0 0 6 6"><path d="M0 0h6L3 6z" fill="currentColor"/></svg>"#;
    El::new("div.m-row")
        .child(El::new(if indent { "div.m-label.t.in" } else { "div.m-label.t" }).text(label))
        .child(
            El::new("div.m-dd")
                .child(El::new("div.m-ring"))
                .child(El::new("button.m-dd-text").id(item(&format!("dropdown_cycle:{key}"))).tab().text(value))
                .child(El::new("button.m-dd-chev").id(item(&format!("dropdown_menu:{key}"))).tab().name(label).svg(chev)),
        )
}

/// The Instrument tab's rows for a candle-family chart, top to bottom.
fn instrument_rows(s: &InstrumentSettings) -> Vec<El> {
    let tick_style = match s.price_tick_style.as_str() {
        "dashed" => t_settings(SettingsKey::TickStyleDash),
        "solid" => t_settings(SettingsKey::TickStyleLine),
        _ => t_settings(SettingsKey::TickStyleDots),
    };
    let lang = current_language();
    vec![
        section(t_settings(SettingsKey::SectionCandles)),
        check("instrument:use_prev_close", s.use_prev_close_color, t_settings(SettingsKey::BodyColorPrevClose)).css("margin-bottom: 8px"),
        check_swatches("instrument:body_enabled", s.body_enabled, t_settings(SettingsKey::Body), ("instrument:body_up_color", &s.body_up_color), ("instrument:body_down_color", &s.body_down_color)),
        check_swatches("instrument:border_enabled", s.border_enabled, t_settings(SettingsKey::Borders), ("instrument:border_up_color", &s.border_up_color), ("instrument:border_down_color", &s.border_down_color)),
        check_swatches("instrument:wick_enabled", s.wick_enabled, t_settings(SettingsKey::Wick), ("instrument:wick_up_color", &s.wick_up_color), ("instrument:wick_down_color", &s.wick_down_color)),
        dropdown("candle_style", "Candle style", &s.candle_style_label, true).css("margin-bottom: 16px"),
        section(t_settings(SettingsKey::SectionDataConfig)),
        check("instrument:show_bar_countdown", s.show_bar_countdown, t_settings(SettingsKey::CountdownToClose)),
        section(t_settings(SettingsKey::SectionPriceTick)),
        check("instrument:price_tick_extend_right", s.price_tick_extend_right, t_settings(SettingsKey::ExtendRight)),
        check("instrument:price_tick_extend_left", s.price_tick_extend_left, t_settings(SettingsKey::ExtendLeft)),
        dropdown("price_tick_style", t_settings(SettingsKey::LineStyle), tick_style, false),
        dropdown("precision", t_settings(SettingsKey::Precision), &s.precision_label, false),
        dropdown("timezone", ClockKey::Timezone.get(lang), &s.timezone_label, false),
        check("instrument:use_24h", s.use_24h, ClockKey::Use24h.get(lang)),
        check("instrument:show_utc_prefix", s.show_utc_prefix, ClockKey::ShowUtcPrefix.get(lang)),
        dropdown("date_format", ClockKey::DateFormat.get(lang), &s.date_format_label, false),
        check("instrument:show_day_of_week", s.show_day_of_week, ClockKey::DayOfWeek.get(lang)),
    ]
}

/// Height of [`instrument_rows`] (the old `total_content_height`).
pub fn content_height() -> f64 {
    // 18 rows of 32 px, plus the 8 px after the first checkbox and 16 px after the
    // candle style.
    18.0 * ROW + 8.0 + 16.0
}

fn tab_icon(tab: ChartSettingsTab) -> &'static str {
    use mlc_app_layout::ui::Icon;
    match tab {
        ChartSettingsTab::Instrument => Icon::Candlestick.svg(),
        ChartSettingsTab::StatusLine => Icon::Legend.svg(),
        ChartSettingsTab::ScalesLines => Icon::Grid.svg(),
        ChartSettingsTab::Appearance => Icon::Palette.svg(),
    }
}

/// The whole tree. `scroll` is the body's current offset.
pub fn tree(state: &ChartSettingsState, s: &InstrumentSettings, at: Rect, scroll: f64) -> El {
    let rail = El::new("div.m-rail").children(ChartSettingsTab::all().iter().map(|t| {
        let cls = if *t == state.active_tab { "button.m-tab.on" } else { "button.m-tab" };
        El::new(cls).id(format!("{ID}:tab:{}", t.id())).tab().name(t.label()).svg(tab_icon(*t))
    }));
    let active = state.active_tab;
    let rows = El::new("div.m-rows").css(format!("margin-top: {}px", -scroll)).children(instrument_rows(s));
    let view = El::new("div.m-view").id(VIEWPORT).child(rows).child_if(content_height() > VIEW_H, || El::new("div.m-scroll").paint_slot(SCROLLBAR));
    let content = El::new("div.m-content").child(El::new("div.m-title").text(active.label())).child(view);
    let body = El::new("div.m-body-row").css("display: flex; flex-direction: row; flex-grow: 1").child(rail).child(content);
    let ok = "OK";
    let template = t_settings(SettingsKey::ButtonTemplate);
    let dialog = domkit::dialog(ID, t_settings(SettingsKey::Title), body, &[ok, template], at);
    let shadow = El::new("div.m-shadow").css(format!("top: {}px; left: {}px; width: {}px; height: {}px", at.y + 4.0, at.x + 4.0, W, H));
    let grip_svg = r#"<svg viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1"><path d="M7 9L9 7M4 9L9 4M1 9L9 1"/></svg>"#;
    let grip = El::new("div.m-grip").css(format!("top: {}px; left: {}px", at.y + 1.0 + H - 10.0, at.x + 1.0 + W - 10.0)).svg(grip_svg);
    El::new("div.k-root.m-root").css("height: 100%").child(shadow).child(dialog).child(grip)
}

/// What a DOM click means to the app: the id under its `chart_settings:` prefix.
pub fn app_id(clicked: &str) -> Option<String> {
    let rest = clicked.strip_prefix("cs:")?;
    Some(match rest {
        "close" => "close".into(),
        "foot:0" => "ok".into(),
        "foot:1" => "template".into(),
        r if r.starts_with("tab:") || r.starts_with("item:") => r.into(),
        _ => return None,
    })
}

/// A checkbox's DOM control (tests read it).
pub fn is_checkbox(c: &Control) -> bool {
    matches!(c, Control::Checkbox { .. })
}
