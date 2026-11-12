//! The preset-name modal (save as / rename / new chart) as a uzor kit dialog.
//!
//! Was: `mlc-app-layout` `layout/modals/preset_name_input.rs`, drawn by hand through
//! `render_modal_shell` with hit rects registered on the app's own coordinator and the
//! clicks decoded in `input/panel_click.rs`. Now: `domkit::dialog` with a kit text
//! input; the framework owns focus, caret, selection, IME and the clipboard. The app
//! keeps the state and the action (`ChartApp::commit_preset_name`).
//!
//! The CSS reproduces the old geometry: a 400 px frame centred in the chart area,
//! 44 px header, 16 px body inset, a 32 px field, a 52 px footer with the primary
//! button hard right; square corners; the app theme's colours.

use mlc_app_layout::ui::modal_settings::PresetNameInputMode;
use uzor::types::Rect;
use uzor::ui::dom::{Edit, El};
use uzor::ui::domkit;

/// The dialog id; its parts are `pni:close`, `pni:scrim`, `pni:foot:0` (save).
pub const ID: &str = "pni";
/// The text field.
pub const FIELD: &str = "pni:name";
/// The save button.
pub const SAVE: &str = "pni:foot:0";
/// The close button.
pub const CLOSE: &str = "pni:close";

const W: f64 = 400.0;
/// Header 44 + body padding 2×16 + field 32 + footer 52.
const H: f64 = 160.0;

/// The colours the old modal took from the app theme.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub bg: String,
    pub line: String,
    pub text: String,
    pub text_hover: String,
    pub muted: String,
    pub field: String,
    pub accent: String,
    pub accent_hover: String,
    pub on_accent: String,
}

impl Look {
    /// The app's current theme, as `render_preset_name_input` resolves it.
    pub fn of(app: &mlc_app::ChartApp) -> Self {
        let ft = app.panel_app.frame_theme_for_render();
        let tt = app.panel_app.toolbar_theme_for_render();
        let wt = mlc_app_layout::layout::render_ui::toolbar_to_widget_theme(&tt, &ft);
        // `ModalTheme::from_frame_theme` leaves the button colours at their defaults.
        let mt = mlc_app_layout::ui::widgets::ModalTheme::default();
        Look {
            bg: ft.modal_bg.clone(),
            line: tt.separator.clone(),
            text: tt.item_text.clone(),
            text_hover: tt.item_text_hover.clone(),
            muted: wt.text_disabled.clone(),
            field: wt.bg_normal.clone(),
            accent: mt.accent,
            accent_hover: mt.accent_hover,
            on_accent: mt.accent_text,
        }
    }

    /// The stylesheet over the kit's; `mode`'s save label sets the button width the
    /// old footer computed.
    pub fn css(&self) -> String {
        let save_w = save_width(save_label());
        let Look { bg, line, text, text_hover, muted, field, accent, accent_hover, on_accent } = self;
        // The old shell centres text on a middle baseline, one pixel lower than the
        // DOM's line box puts it: the 2 px top paddings below. The close icon sits
        // 12 px in from the frame (head padding 6 + the 26 px button's 5).
        // The frame's own box stays transparent so the outer half of its border lies
        // over what is behind, as the old stroke did; the parts carry the fill. The
        // old hard shadow (3 px right and down) is a box of its own: uzor's
        // `box-shadow` has no x offset.
        // The old modal stroked 1 px lines on whole-pixel coordinates: each covers half
        // of the two pixel rows around it. A 2 px border at half the colour's alpha,
        // one pixel earlier, paints the very same two rows.
        let (r, g, b, a) = mlc_core::parse_css_color(line);
        let half = format!("rgba({r},{g},{b},{:.3})", a as f64 / 255.0 * 0.5);
        format!(
            "div.m-root {{ font-family: sans-serif }}
div.k-scrim {{ background: transparent }}
div.k-dialog {{ border-radius: 0; background: transparent; border: 2px solid {half}; box-shadow: none }}
div.k-dialog-head, div.k-dialog-body, div.k-dialog-foot {{ background: {bg} }}
div.m-shadow {{ position: fixed; z-index: 145; background: rgba(0,0,0,0.4) }}
div.k-dialog-head {{ height: 44px; padding: 0 6px 0 15px; font-size: 13px; font-weight: 400; color: {text}; border-bottom: 2px solid {half} }}
div.k-dialog-head > div.k-dialog-title {{ padding-top: 2px }}
div.k-dialog-head > button.k-act {{ color: {text} }}
div.k-dialog-head > button.k-act:hover {{ background: transparent; color: {text_hover} }}
div.k-dialog-body {{ padding: 14px 14px 0 14px; color: {text}; gap: 0 }}
div.k-input {{ height: 34px; padding: 2px 7px 0 7px; border-radius: 4px; background: {field}; border: 2px solid {half}; font-size: 13px; color: {text}; --placeholder: {muted}; --caret: {text} }}
div.k-input:hover {{ border-color: {half} }}
div.k-input:focus {{ border-color: {half} }}
div.k-dialog-foot {{ height: 52px; padding: 0 15px; align-items: center; border-top: 2px solid {half} }}
button.k-btn.primary {{ width: {save_w}px; height: 32px; padding: 2px 0 0 0; border-radius: 0; font-weight: 400; background: {accent}; color: {on_accent}; text-align: center }}
button.k-btn.primary:hover {{ background: {accent_hover} }}"
        )
    }
}

/// The primary button's label (the app's i18n "Save").
pub fn save_label() -> &'static str {
    mlc_core::i18n::TextKey::Save.get(mlc_core::i18n::current_language())
}

/// The old footer's width rule for a creator button.
pub fn save_width(label: &str) -> f64 {
    70.0f64.max(label.chars().count() as f64 * 8.0 + 24.0)
}

/// The title the old modal showed for `mode` (English only there too).
pub fn title(mode: PresetNameInputMode) -> &'static str {
    match mode {
        PresetNameInputMode::SaveAs => "Save As",
        PresetNameInputMode::Rename => "Rename",
        PresetNameInputMode::NewChart => "New Chart",
        PresetNameInputMode::CreateIndicatorSet => "Create Indicator Set",
    }
}

/// Where the frame goes: centred in the chart area (below the chrome strip), or where
/// the user dragged it, clamped on screen as the shell does.
/// (The box is one pixel larger all round than the old frame rect: see [`Look::css`].)
pub fn frame(window: Rect, chrome_h: f64, position: Option<(f64, f64)>) -> Rect {
    let area = Rect::new(window.x, window.y + chrome_h, window.width, (window.height - chrome_h).max(0.0));
    let (cx, cy) = ((area.width - W) / 2.0, (area.height - H) / 2.0);
    let (x, y) = position.unwrap_or((cx, cy));
    let clamp = |p: f64, size: f64, screen: f64| if size >= screen { 0.0 } else { p.clamp(0.0, screen - size) };
    Rect::new(area.x + clamp(x, W, area.width) - 1.0, area.y + clamp(y, H, area.height) - 1.0, W + 2.0, H + 2.0)
}

/// The tree: the kit dialog with one kit text field and the save button.
pub fn tree(mode: PresetNameInputMode, name: &str, at: Rect) -> El {
    let save = save_label();
    let body = domkit::input(FIELD, Edit::new(name).placeholder("Preset name...")).name("Preset name");
    let dialog = domkit::dialog(ID, title(mode), body, &[save], at);
    let shadow = El::new("div.m-shadow").css(format!("top: {}px; left: {}px; width: {}px; height: {}px", at.y + 4.0, at.x + 4.0, at.width - 2.0, at.height - 2.0));
    El::new("div.k-root.m-root").css("height: 100%").child(shadow).child(dialog)
}

/// What the user did in the dialog this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// Save with this name.
    Commit(String),
    /// Close without saving.
    Close,
}

/// Read a frame's clicks.
pub fn outcome(clicked: Option<&str>, name: &str) -> Option<Outcome> {
    match clicked? {
        SAVE => Some(Outcome::Commit(name.to_string())),
        CLOSE => Some(Outcome::Close),
        _ => None,
    }
}
