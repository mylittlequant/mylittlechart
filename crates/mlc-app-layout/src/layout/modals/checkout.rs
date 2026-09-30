//! Checkout modal — plans, the anonymous key gate, the pay step, success.
//!
//! Drawn from [`CheckoutState`]; nothing here decides anything. Spacing,
//! typography and colour follow the settings modal (`user_settings.rs`): a
//! 20 px inset, `bold 14px` headings, `11px` muted labels, 26 px controls with
//! the settings radius, every colour from [`ToolbarTheme`] / [`ModalTheme`].
//! The one exception is the QR code, which is dark-on-light on purpose (see
//! `crate::ui::qr_svg::QR_DARK`).
//!
//! The body is laid out twice per frame by the same code: once measuring, to
//! size the modal, and once drawing into the shell. That keeps the frame
//! exactly as tall as its content for every step and every translation.
//!
//! The pay step is [`draw_pay`], one component with one argument list; the
//! processor's hosted widget can take its place without touching the rest.

use mlc_core::engine::render::{RenderContext, TextAlign, TextBaseline};
use mlc_core::i18n::{current_language, CheckoutKey, Language};
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::types::Rect as WidgetRect;
use uzor::WidgetId;

use crate::layout::render_chart::FrameTheme;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::ui::checkout::{
    fill, format_countdown, format_usd_micros, AnonSession, CheckoutError, CheckoutState,
    CheckoutStep, CopyTarget, InvoiceNote, InvoicePhase, PayOptionUi, PlanKind, PlanStatus, PlanUi, SignInPhase,
};
use crate::ui::qr_svg::{QrMatrix, QR_DARK, QR_LIGHT};
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::z_order::ZLayer;
use crate::ui::widgets::{
    draw_input, draw_input_cursor, fit_text_to_width, render_modal_shell, InputConfig, ModalFooter, ModalShell,
    ModalShellSpec, ModalTheme, WidgetState, SETTINGS_BUTTON_RADIUS,
};

const MODAL_W: f64 = 460.0;
const PAD: f64 = 20.0;
const CARD_MIN_H: f64 = 52.0;
const CARD_GAP: f64 = 8.0;
const CARD_PAD: f64 = 12.0;
const ACTION_W: f64 = 80.0;
const CTRL_H: f64 = 26.0;
const BTN_H: f64 = 30.0;
const CHIP_H: f64 = 26.0;
const CHIP_GAP: f64 = 6.0;
const QR_SIDE: f64 = 152.0;
const SMALL_BTN_W: f64 = 60.0;
const SMALL_BTN_H: f64 = 22.0;
const FIELD_H: f64 = 32.0;

/// Text-field id of the account-key box. A literal, not a shared constant:
/// this crate cannot depend on `mlc-app`, where the field is registered
/// (`mlc_app::text_input::CHECKOUT_KEY_INPUT` holds the other half of the pair).
const KEY_FIELD_ID: &str = "checkout_key_input";

const FONT_HEADING: &str = "bold 14px sans-serif";
const FONT_BODY: &str = "13px sans-serif";
const FONT_STRONG: &str = "600 13px sans-serif";
const FONT_SMALL: &str = "11px sans-serif";
const FONT_SMALL_STRONG: &str = "600 11px sans-serif";
const FONT_MONO: &str = "12px monospace";
const FONT_MONO_KEY: &str = "600 14px monospace";

/// What the modal reports back to the frame.
#[derive(Clone, Debug, Default)]
pub struct CheckoutResult {
    /// The frame.
    pub modal_rect: WidgetRect,
    /// Every widget the body registered, with its rect. The click handler
    /// routes on the id; tests read the ids.
    pub hits: Vec<(String, WidgetRect)>,
    /// The account-key box (key-entry page only): its rect and per-character
    /// x positions, which the input side needs for caret and selection.
    pub input_rect: WidgetRect,
    pub char_x_positions: Vec<f64>,
}

/// Draw the checkout modal.
pub fn render_checkout_modal(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    screen_h: f64,
    state: &CheckoutState,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    current_time_ms: u64,
    input_coordinator: &mut InputCoordinator,
) -> CheckoutResult {
    let lang = current_language();
    let theme = ModalTheme::from_toolbar(&frame_theme.modal_bg, &toolbar_theme.separator, toolbar_theme);
    let content_w = MODAL_W - PAD * 2.0;

    let measured = {
        let mut canvas = Canvas::new(&mut *ctx, None, toolbar_theme, frame_theme, &theme, lang, current_time_ms);
        draw_body(&mut canvas, state, 0.0, 0.0, content_w)
    };

    let spec = ModalShellSpec {
        draggable: false,
        footer: ModalFooter::None,
        padding: PAD,
        // Above the modal that opened it: a same-z tie would go to whichever
        // layer was pushed first in the frame.
        layer: ZLayer::ModalOverlay,
        ..ModalShellSpec::new("checkout", CheckoutKey::Title.get(lang), MODAL_W, measured)
    };
    let mut hits = Vec::new();
    let mut field: Option<(WidgetRect, Vec<f64>)> = None;
    let shell = render_modal_shell(
        ctx,
        input_coordinator,
        &spec,
        &theme,
        (screen_w, screen_h),
        |ctx, coord, layer_id, shell: &ModalShell| {
            let mut canvas =
                Canvas::new(ctx, Some((coord, layer_id)), toolbar_theme, frame_theme, &theme, lang, current_time_ms);
            draw_body(&mut canvas, state, shell.content.x, shell.content.y, shell.content.width);
            hits = canvas.hits;
            field = canvas.field;
        },
    );
    let (input_rect, char_x_positions) = field.unwrap_or_default();
    CheckoutResult { modal_rect: shell.frame, hits, input_rect, char_x_positions }
}

// =============================================================================
// Entry points
// =============================================================================

/// Widget id every entry-point button registers. The click handler opens the
/// modal on it.
pub const OPEN_WIDGET_ID: &str = "checkout:open";

/// A small call-to-action for the surfaces that show the plan status or say a
/// feature needs a paid plan. Draws nothing (and returns `None`) while the
/// shell has not switched the checkout on, so no shell without a driver ever
/// shows a dead button.
pub fn draw_entry_button(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    label: &str,
    tt: &ToolbarTheme,
) -> Option<WidgetRect> {
    if !crate::ui::checkout::entry_points_enabled() {
        return None;
    }
    let w = crate::ui::widgets::auto_btn_width(ctx, label, FONT_SMALL_STRONG, 10.0, 60.0);
    let rect = WidgetRect::new(x, y, w, 24.0);
    let hovered = coord.is_hovered(&WidgetId::from(OPEN_WIDGET_ID));
    let bg = if hovered { &tt.accent_hover } else { &tt.accent };
    ctx.set_fill_color(bg);
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    ctx.set_font(FONT_SMALL_STRONG);
    ctx.set_fill_color(&tt.item_text_active);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, rect.center_x(), rect.center_y());
    ctx.set_text_align(TextAlign::Left);
    coord.register_on_layer(OPEN_WIDGET_ID, rect, Sense::CLICK | Sense::HOVER, layer_id);
    Some(rect)
}

/// The same entry as a quiet text link, centred on `cx` — for the sign-in
/// overlay, where a filled button would compete with the provider buttons.
/// Draws nothing while the shell has not switched the checkout on.
pub fn draw_entry_link(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    cx: f64,
    y: f64,
    label: &str,
    tt: &ToolbarTheme,
) {
    if !crate::ui::checkout::entry_points_enabled() {
        return;
    }
    ctx.set_font(FONT_SMALL_STRONG);
    let w = ctx.measure_text(label);
    let x = cx - w / 2.0;
    let hovered = coord.is_hovered(&WidgetId::from(OPEN_WIDGET_ID));
    let color = if hovered { &tt.accent_hover } else { &tt.accent };
    ctx.set_fill_color(color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(label, x, y);
    if hovered {
        ctx.set_stroke_color(color);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(x, y + 14.0);
        ctx.line_to(x + w, y + 14.0);
        ctx.stroke();
    }
    coord.register_on_layer(OPEN_WIDGET_ID, WidgetRect::new(x - 4.0, y - 3.0, w + 8.0, 20.0), Sense::CLICK | Sense::HOVER, layer_id);
}

// =============================================================================
// Canvas: one code path for measuring and drawing
// =============================================================================

struct Canvas<'a> {
    ctx: &'a mut dyn RenderContext,
    /// `None` while measuring.
    input: Option<(&'a mut InputCoordinator, &'a LayerId)>,
    tt: &'a ToolbarTheme,
    ft: &'a FrameTheme,
    theme: &'a ModalTheme,
    lang: Language,
    now_ms: u64,
    hits: Vec<(String, WidgetRect)>,
    /// The account-key box, once drawn.
    field: Option<(WidgetRect, Vec<f64>)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Btn {
    Primary,
    Secondary,
}

impl<'a> Canvas<'a> {
    fn new(
        ctx: &'a mut dyn RenderContext,
        input: Option<(&'a mut InputCoordinator, &'a LayerId)>,
        tt: &'a ToolbarTheme,
        ft: &'a FrameTheme,
        theme: &'a ModalTheme,
        lang: Language,
        now_ms: u64,
    ) -> Self {
        Self { ctx, input, tt, ft, theme, lang, now_ms, hits: Vec::new(), field: None }
    }

    fn drawing(&self) -> bool {
        self.input.is_some()
    }

    fn t(&self, key: CheckoutKey) -> &'static str {
        key.get(self.lang)
    }

    fn width_of(&mut self, text: &str, font: &str) -> f64 {
        self.ctx.set_font(font);
        self.ctx.measure_text(text)
    }

    fn wrap(&mut self, text: &str, font: &str, max_w: f64) -> Vec<String> {
        self.ctx.set_font(font);
        crate::ui::widgets::wrap_text_to_width(&*self.ctx, text, max_w)
    }

    /// Wrap by characters (addresses and keys have no spaces to break on).
    fn wrap_chars(&mut self, text: &str, font: &str, max_w: f64, max_lines: usize) -> Vec<String> {
        self.ctx.set_font(font);
        let mut lines: Vec<String> = Vec::new();
        let mut current = String::new();
        for ch in text.chars() {
            current.push(ch);
            if self.ctx.measure_text(&current) > max_w && current.chars().count() > 1 {
                current.pop();
                lines.push(std::mem::take(&mut current));
                current.push(ch);
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
        if lines.len() > max_lines {
            lines.truncate(max_lines);
            if let Some(last) = lines.last_mut() {
                *last = fit_text_to_width(&*self.ctx, &format!("{last}\u{2026}"), max_w);
            }
        }
        lines
    }

    fn text(&mut self, s: &str, x: f64, y: f64, font: &str, color: &str, align: TextAlign, base: TextBaseline) {
        if !self.drawing() || s.is_empty() {
            return;
        }
        self.ctx.set_font(font);
        self.ctx.set_fill_color(color);
        self.ctx.set_text_align(align);
        self.ctx.set_text_baseline(base);
        self.ctx.fill_text(s, x, y);
        self.ctx.set_text_align(TextAlign::Left);
    }

    /// Left-aligned, top-baselined text.
    fn label(&mut self, s: &str, x: f64, y: f64, font: &str, color: &str) {
        self.text(s, x, y, font, color, TextAlign::Left, TextBaseline::Top);
    }

    fn register(&mut self, id: &str, rect: WidgetRect) {
        if let Some((coord, layer)) = self.input.as_mut() {
            coord.register_on_layer(format!("checkout:{id}").as_str(), rect, Sense::CLICK | Sense::HOVER, *layer);
            self.hits.push((id.to_string(), rect));
        }
    }

    fn hovered(&self, id: &str) -> bool {
        self.input
            .as_ref()
            .is_some_and(|(coord, _)| coord.is_hovered(&WidgetId::from(format!("checkout:{id}").as_str())))
    }

    fn muted(&self) -> &'a str {
        let tt: &'a ToolbarTheme = self.tt;
        &tt.item_text_muted
    }

    fn strong(&self) -> &'a str {
        let tt: &'a ToolbarTheme = self.tt;
        &tt.item_text
    }

    /// Paint a rounded button. `live == false` draws it dead.
    fn paint_button(&mut self, rect: WidgetRect, label: &str, kind: Btn, live: bool, hovered: bool) {
        if !self.drawing() {
            return;
        }
        let (tt, theme) = (self.tt, self.theme);
        let (bg, fg, border) = match (kind, live) {
            (_, false) => (tt.item_bg_hover.as_str(), tt.item_text_muted.as_str(), None),
            (Btn::Primary, true) => (
                if hovered { theme.accent_hover.as_str() } else { theme.accent.as_str() },
                theme.accent_text.as_str(),
                None,
            ),
            (Btn::Secondary, true) => (
                if hovered { tt.button_bg_hover.as_str() } else { tt.dropdown_bg.as_str() },
                tt.item_text.as_str(),
                Some(tt.separator.as_str()),
            ),
        };
        self.ctx.set_fill_color(bg);
        self.ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
        if let Some(border) = border {
            self.ctx.set_stroke_color(border);
            self.ctx.set_stroke_width(1.0);
            self.ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
        }
        self.ctx.set_font(FONT_STRONG);
        let fitted = fit_text_to_width(&*self.ctx, label, (rect.width - 12.0).max(1.0));
        self.text(&fitted, rect.center_x(), rect.center_y(), FONT_STRONG, fg, TextAlign::Center, TextBaseline::Middle);
    }

    /// A rounded button with a label. `enabled == false` draws it dead and
    /// registers nothing.
    fn button(&mut self, id: &str, rect: WidgetRect, label: &str, kind: Btn, enabled: bool) {
        if !self.drawing() {
            return;
        }
        let hovered = enabled && self.hovered(id);
        self.paint_button(rect, label, kind, enabled, hovered);
        if enabled {
            self.register(id, rect);
        }
    }

    /// A small 11 px button (copy).
    fn small_button(&mut self, id: &str, rect: WidgetRect, label: &str, active: bool) {
        if !self.drawing() {
            return;
        }
        let hovered = self.hovered(id);
        let tt = self.tt;
        let bg = if hovered { tt.button_bg_hover.as_str() } else { tt.dropdown_bg.as_str() };
        self.ctx.set_fill_color(bg);
        self.ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
        self.ctx.set_stroke_color(&tt.separator);
        self.ctx.set_stroke_width(1.0);
        self.ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
        let color = if active { tt.success.as_str() } else { tt.item_text.as_str() };
        let fitted = {
            self.ctx.set_font(FONT_SMALL);
            fit_text_to_width(&*self.ctx, label, (rect.width - 8.0).max(1.0))
        };
        self.text(&fitted, rect.center_x(), rect.center_y(), FONT_SMALL, color, TextAlign::Center, TextBaseline::Middle);
        self.register(id, rect);
    }

    /// A text link; returns its width.
    fn link(&mut self, id: &str, s: &str, x: f64, y: f64, right_aligned_to: Option<f64>) -> f64 {
        let w = self.width_of(s, FONT_SMALL_STRONG);
        let lx = right_aligned_to.map_or(x, |r| r - w);
        if self.drawing() {
            let hovered = self.hovered(id);
            let tt = self.tt;
            let color = if hovered { tt.accent_hover.as_str() } else { tt.accent.as_str() };
            self.label(s, lx, y, FONT_SMALL_STRONG, color);
            if hovered {
                self.ctx.set_stroke_color(color);
                self.ctx.set_stroke_width(1.0);
                self.ctx.begin_path();
                self.ctx.move_to(lx, y + 14.0);
                self.ctx.line_to(lx + w, y + 14.0);
                self.ctx.stroke();
            }
            self.register(id, WidgetRect::new(lx - 2.0, y - 3.0, w + 4.0, 20.0));
        }
        w
    }

    /// The account-key box: focused for as long as the page is up, caret and
    /// selection from the shared text-field store.
    fn text_field(&mut self, rect: WidgetRect) {
        if !self.drawing() {
            return;
        }
        let (tt, ft, now) = (self.tt, self.ft, self.now_ms);
        let placeholder = CheckoutKey::KeyEntryPlaceholder.get(self.lang);
        let ctx = &mut *self.ctx;
        let Some((coord, layer)) = self.input.as_mut() else { return };
        let wid = WidgetId::from(KEY_FIELD_ID);
        if !coord.text_fields().is_focused(&wid) {
            coord.text_fields_mut().focus(wid.clone());
        }
        let text = coord.text_fields().text(&wid).to_string();
        let cursor = coord.text_fields().cursor(&wid);
        let (sel_start, sel_end) = match coord.text_fields().selection_range(&wid) {
            Some((lo, hi)) => (Some(lo), Some(hi)),
            None => (None, None),
        };
        let engaged = coord.text_fields().is_engaged(&wid);
        let config = InputConfig::new(&text)
            .with_focused(engaged)
            .with_cursor(cursor)
            .with_placeholder(placeholder)
            .with_selection(sel_start, sel_end);
        let drawn = draw_input(ctx, &config, WidgetState::Normal, rect, &toolbar_to_widget_theme(tt, ft));
        if engaged && coord.text_fields().cursor_visible(now) {
            draw_input_cursor(ctx, drawn.cursor_x, drawn.cursor_y, drawn.cursor_height, &tt.item_text);
        }
        coord.register_on_layer("checkout:key_input", rect, Sense::CLICK | Sense::DRAG, *layer);
        self.hits.push(("key_input".to_string(), rect));
        self.field = Some((rect, drawn.char_x_positions));
    }

    fn rounded_box(&mut self, rect: WidgetRect, fill_color: &str, border: &str) {
        if !self.drawing() {
            return;
        }
        self.ctx.set_fill_color(fill_color);
        self.ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
        self.ctx.set_stroke_color(border);
        self.ctx.set_stroke_width(1.0);
        self.ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    }

    /// A check mark inside a `size` box at (`x`, `y`).
    fn check_mark(&mut self, x: f64, y: f64, size: f64, color: &str, width: f64) {
        if !self.drawing() {
            return;
        }
        self.ctx.set_stroke_color(color);
        self.ctx.set_stroke_width(width);
        self.ctx.set_line_cap("round");
        self.ctx.set_line_join("round");
        self.ctx.begin_path();
        self.ctx.move_to(x + size * 0.18, y + size * 0.54);
        self.ctx.line_to(x + size * 0.42, y + size * 0.78);
        self.ctx.line_to(x + size * 0.84, y + size * 0.24);
        self.ctx.stroke();
    }
}

// =============================================================================
// Body
// =============================================================================

/// Draw (or measure) the body for `state.step` at (`x`, `y`) in a column of
/// width `w`. Returns the height used.
fn draw_body(c: &mut Canvas<'_>, st: &CheckoutState, x: f64, y: f64, w: f64) -> f64 {
    let used = match st.step {
        CheckoutStep::Plans => draw_plans(c, st, x, y, w),
        CheckoutStep::KeyGate => draw_key_gate(c, st, x, y, w),
        CheckoutStep::Pay => draw_pay(c, st, x, y, w),
        CheckoutStep::Paid => draw_paid(c, st, x, y, w),
        CheckoutStep::Scheduled => draw_scheduled(c, st, x, y, w),
        CheckoutStep::KeyEntry => draw_key_entry(c, st, x, y, w),
    };
    used.max(120.0)
}

fn error_lines(c: &mut Canvas<'_>, error: &CheckoutError, x: f64, y: f64, w: f64) -> f64 {
    let message = error.message(c.lang);
    let lines = c.wrap(&message, FONT_SMALL, w);
    let danger = c.tt.danger.clone();
    for (i, line) in lines.iter().enumerate() {
        c.label(line, x, y + i as f64 * 15.0, FONT_SMALL, &danger);
    }
    lines.len() as f64 * 15.0
}

fn heading(c: &mut Canvas<'_>, s: &str, x: f64, y: f64) -> f64 {
    let color = c.strong().to_string();
    c.label(s, x, y, FONT_HEADING, &color);
    24.0
}

// ---- plans ------------------------------------------------------------------

fn plan_name(plan: &PlanUi, lang: Language) -> String {
    match plan.kind {
        PlanKind::PaidAccount => CheckoutKey::PlanPaid.get(lang).to_string(),
        PlanKind::AnonAccount => CheckoutKey::PlanAnon.get(lang).to_string(),
        PlanKind::Quant => {
            let tier = plan.tier.map_or_else(|| format!("${}", plan.price_usd), |t| t.to_string());
            fill(CheckoutKey::PlanQuantTier.get(lang), &[("tier", &tier)])
        }
    }
}

fn plan_desc(plan: &PlanUi, lang: Language) -> String {
    match plan.kind {
        PlanKind::PaidAccount => CheckoutKey::PlanPaidDesc.get(lang).to_string(),
        PlanKind::AnonAccount => CheckoutKey::PlanAnonDesc.get(lang).to_string(),
        PlanKind::Quant => match plan.quota {
            Some(q) => fill(CheckoutKey::PlanQuantDescQuota.get(lang), &[("quota", &q.to_string())]),
            None => CheckoutKey::PlanQuantDesc.get(lang).to_string(),
        },
    }
}

fn draw_plans(c: &mut Canvas<'_>, st: &CheckoutState, x: f64, y: f64, w: f64) -> f64 {
    let mut cy = y;
    cy += heading(c, c.t(CheckoutKey::PlansHeading), x, cy);

    let plans = st.visible_plans();
    if plans.is_empty() {
        if st.plans_loading || !st.plans_loaded {
            let muted = c.muted().to_string();
            c.label(c.t(CheckoutKey::LoadingPlans), x, cy, FONT_BODY, &muted);
            cy += 24.0;
        }
    } else {
        let accounts: Vec<&PlanUi> = plans.iter().copied().filter(|p| p.kind != PlanKind::Quant).collect();
        let tiers: Vec<&PlanUi> = plans.iter().copied().filter(|p| p.kind == PlanKind::Quant).collect();
        for (title, group) in [(CheckoutKey::SectionAccount, accounts), (CheckoutKey::SectionQuant, tiers)] {
            if group.is_empty() {
                continue;
            }
            let muted = mlc_core::apply_opacity(&c.tt.item_text, 0.75);
            c.label(&c.t(title).to_uppercase(), x, cy, FONT_SMALL, &muted);
            cy += 20.0;
            for plan in group {
                cy += draw_plan_card(c, st, plan, x, cy, w) + CARD_GAP;
            }
            cy += 4.0;
        }
    }

    if let Some(error) = &st.error {
        cy += error_lines(c, error, x, cy, w) + 6.0;
        if !st.plans_loading && st.plans.is_empty() {
            let rect = WidgetRect::new(x, cy, 130.0, BTN_H);
            c.button("retry", rect, c.t(CheckoutKey::Retry), Btn::Secondary, true);
            cy += BTN_H + 8.0;
        }
    }

    cy += 4.0;
    c.link("have_code", c.t(CheckoutKey::HaveCode), x, cy, None);
    if st.have_key_shown() {
        // Bought an anonymous account earlier? The key gets back into it.
        c.link("have_key", c.t(CheckoutKey::HaveKey), x, cy, Some(x + w));
    }
    cy += 18.0;
    cy - y
}

fn draw_plan_card(c: &mut Canvas<'_>, st: &CheckoutState, plan: &PlanUi, x: f64, y: f64, w: f64) -> f64 {
    let lang = c.lang;
    let status = st.plan_status(plan);
    let busy = st.starting.as_deref() == Some(plan.id.as_str());
    let any_busy = st.starting.is_some();

    let name = plan_name(plan, lang);
    let desc = plan_desc(plan, lang);
    let price = format!("${}", plan.price_usd);
    let suffix = if plan.monthly { c.t(CheckoutKey::PerMonth) } else { c.t(CheckoutKey::PerOneTime) };

    let price_w = c.width_of(&price, FONT_HEADING).max(c.width_of(suffix, FONT_SMALL));
    let action_x = x + w - CARD_PAD - ACTION_W;
    let price_right = action_x - 12.0;
    let text_w = (price_right - price_w - 12.0 - (x + CARD_PAD)).max(80.0);

    let desc_lines: Vec<String> = c.wrap(&desc, FONT_SMALL, text_w).into_iter().take(3).collect();
    let h = (CARD_PAD + 18.0 + desc_lines.len() as f64 * 14.0 + CARD_PAD).max(CARD_MIN_H);
    if !c.drawing() {
        return h;
    }

    let rect = WidgetRect::new(x, y, w, h);
    let actionable = status.is_actionable() && !any_busy && st.checkout_enabled;
    let hovered = actionable && c.hovered(&format!("plan:{}", plan.id));
    let card_bg = if hovered { c.tt.button_bg_hover.clone() } else { c.tt.dropdown_bg.clone() };
    let border = c.tt.separator.clone();
    c.rounded_box(rect, &card_bg, &border);

    // Name, with a Tor tag on the onion-only plans.
    let name_color = if status.is_actionable() { c.tt.item_text.clone() } else { c.tt.item_text_muted.clone() };
    c.label(&name, x + CARD_PAD, y + CARD_PAD, FONT_STRONG, &name_color);
    if plan.onion_only {
        let nw = c.width_of(&name, FONT_STRONG);
        let tag = c.t(CheckoutKey::TorTag);
        let tw = c.width_of(tag, FONT_SMALL_STRONG) + 10.0;
        let tag_rect = WidgetRect::new(x + CARD_PAD + nw + 8.0, y + CARD_PAD, tw, 16.0);
        let accent = c.tt.accent.clone();
        c.ctx.set_stroke_color(&accent);
        c.ctx.set_stroke_width(1.0);
        c.ctx.stroke_rounded_rect(tag_rect.x, tag_rect.y, tag_rect.width, tag_rect.height, 3.0);
        c.text(tag, tag_rect.center_x(), tag_rect.center_y(), FONT_SMALL_STRONG, &accent, TextAlign::Center, TextBaseline::Middle);
    }
    let muted = c.muted().to_string();
    for (i, line) in desc_lines.iter().enumerate() {
        c.label(line, x + CARD_PAD, y + CARD_PAD + 18.0 + i as f64 * 14.0, FONT_SMALL, &muted);
    }

    // Price column.
    let price_color = name_color.clone();
    c.text(&price, price_right, y + CARD_PAD, FONT_HEADING, &price_color, TextAlign::Right, TextBaseline::Top);
    c.text(suffix, price_right, y + CARD_PAD + 20.0, FONT_SMALL, &muted, TextAlign::Right, TextBaseline::Top);

    // Action / badge column.
    let action_rect = WidgetRect::new(action_x, y + (h - CTRL_H) / 2.0, ACTION_W, CTRL_H);
    match status {
        PlanStatus::Available | PlanStatus::Upgrade | PlanStatus::Downgrade => {
            let label = match (status, plan.kind) {
                (PlanStatus::Upgrade, _) => c.t(CheckoutKey::ActionUpgrade),
                (PlanStatus::Downgrade, _) => c.t(CheckoutKey::ActionDowngrade),
                (_, PlanKind::Quant) => c.t(CheckoutKey::Subscribe),
                _ => c.t(CheckoutKey::Buy),
            };
            let kind = if status == PlanStatus::Downgrade { Btn::Secondary } else { Btn::Primary };
            if busy {
                c.paint_button(action_rect, "\u{2026}", kind, false, false);
            } else {
                // The card is the click target; the button is its drawn face.
                c.paint_button(action_rect, label, kind, actionable, hovered);
                if actionable {
                    c.register(&format!("plan:{}", plan.id), rect);
                }
            }
        }
        PlanStatus::Owned | PlanStatus::Current => {
            let key = if status == PlanStatus::Owned { CheckoutKey::Owned } else { CheckoutKey::Current };
            let success = c.tt.success.clone();
            let label = format!("\u{2713} {}", c.t(key));
            c.text(&label, action_rect.right(), action_rect.center_y(), FONT_SMALL_STRONG, &success, TextAlign::Right, TextBaseline::Middle);
        }
        PlanStatus::Scheduled => {
            let warn = c.tt.warning.clone();
            c.text(c.t(CheckoutKey::Scheduled), action_rect.right(), action_rect.center_y(), FONT_SMALL_STRONG, &warn, TextAlign::Right, TextBaseline::Middle);
        }
    }
    h
}

// ---- key gate ---------------------------------------------------------------

fn draw_key_row(c: &mut Canvas<'_>, anon: &AnonSession, copied: bool, x: f64, y: f64, w: f64) -> f64 {
    let box_h = 44.0;
    let box_rect = WidgetRect::new(x, y, w, box_h);
    let bg = c.tt.dropdown_bg.clone();
    let border = c.tt.separator.clone();
    c.rounded_box(box_rect, &bg, &border);
    let btn_w = SMALL_BTN_W;
    let key_w = w - btn_w - 28.0;
    c.ctx.set_font(FONT_MONO_KEY);
    let key = fit_text_to_width(&*c.ctx, &anon.account_key, key_w.max(40.0));
    let strong = c.strong().to_string();
    c.text(&key, x + 12.0, y + box_h / 2.0, FONT_MONO_KEY, &strong, TextAlign::Left, TextBaseline::Middle);
    let btn = WidgetRect::new(x + w - btn_w - 10.0, y + (box_h - SMALL_BTN_H) / 2.0, btn_w, SMALL_BTN_H);
    let label = if copied { c.t(CheckoutKey::Copied) } else { c.t(CheckoutKey::Copy) };
    c.small_button("copy_key", btn, label, copied);
    box_h
}

fn draw_key_gate(c: &mut Canvas<'_>, st: &CheckoutState, x: f64, y: f64, w: f64) -> f64 {
    let Some(anon) = st.anon.as_ref() else { return 0.0 };
    let mut cy = y;
    cy += heading(c, c.t(CheckoutKey::KeyHeading), x, cy);

    let muted = c.muted().to_string();
    let body = c.wrap(c.t(CheckoutKey::KeyBody), FONT_SMALL, w);
    for (i, line) in body.iter().enumerate() {
        c.label(line, x, cy + i as f64 * 15.0, FONT_SMALL, &muted);
    }
    cy += body.len() as f64 * 15.0 + 12.0;

    cy += draw_key_row(c, anon, st.is_copied(CopyTarget::Key), x, cy, w) + 16.0;

    // "I saved my account key" checkbox.
    let row = WidgetRect::new(x, cy, w, 22.0);
    if c.drawing() {
        let box_rect = WidgetRect::new(x, cy + 3.0, 16.0, 16.0);
        let (fill_color, border) = if anon.saved {
            (c.tt.accent.clone(), c.tt.accent.clone())
        } else {
            (c.tt.dropdown_bg.clone(), c.tt.separator.clone())
        };
        c.rounded_box(box_rect, &fill_color, &border);
        if anon.saved {
            let on_accent = c.theme.accent_text.clone();
            c.check_mark(box_rect.x, box_rect.y, 16.0, &on_accent, 2.0);
        }
        let text_color = c.strong().to_string();
        c.label(c.t(CheckoutKey::KeySaved), x + 26.0, cy + 4.0, FONT_BODY, &text_color);
        c.register("key_saved", row);
    }
    cy += 22.0 + 20.0;

    let half = (w - 10.0) / 2.0;
    c.button("back", WidgetRect::new(x, cy, half, BTN_H), c.t(CheckoutKey::Back), Btn::Secondary, true);
    c.button(
        "key_continue",
        WidgetRect::new(x + half + 10.0, cy, half, BTN_H),
        c.t(CheckoutKey::KeyContinue),
        Btn::Primary,
        anon.saved,
    );
    cy += BTN_H;
    cy - y
}

// ---- key entry --------------------------------------------------------------

fn draw_key_entry(c: &mut Canvas<'_>, st: &CheckoutState, x: f64, y: f64, w: f64) -> f64 {
    let mut cy = y;
    cy += heading(c, c.t(CheckoutKey::KeyEntryHeading), x, cy);
    let muted = c.muted().to_string();
    let body = c.wrap(c.t(CheckoutKey::KeyEntryBody), FONT_SMALL, w);
    for (i, line) in body.iter().enumerate() {
        c.label(line, x, cy + i as f64 * 15.0, FONT_SMALL, &muted);
    }
    cy += body.len() as f64 * 15.0 + 12.0;

    c.text_field(WidgetRect::new(x, cy, w, FIELD_H));
    cy += FIELD_H + 10.0;

    if let Some(error) = &st.error {
        cy += error_lines(c, error, x, cy, w) + 6.0;
    }
    cy += 6.0;

    let half = (w - 10.0) / 2.0;
    c.button("back", WidgetRect::new(x, cy, half, BTN_H), c.t(CheckoutKey::Back), Btn::Secondary, true);
    let label = if st.key_working { "\u{2026}" } else { c.t(CheckoutKey::SignIn) };
    c.button("key_submit", WidgetRect::new(x + half + 10.0, cy, half, BTN_H), label, Btn::Primary, !st.key_working);
    cy += BTN_H;
    cy - y
}

// ---- pay --------------------------------------------------------------------

fn find_plan<'s>(st: &'s CheckoutState, plan_id: &str) -> Option<&'s PlanUi> {
    st.plans.iter().find(|p| p.id == plan_id)
}

/// The pay step. One component: the invoice, the chips from `pay[]`, the
/// amount and address with copy buttons, the QR, the countdown, the status.
fn draw_pay(c: &mut Canvas<'_>, st: &CheckoutState, x: f64, y: f64, w: f64) -> f64 {
    let Some(inv) = st.invoice.as_ref() else { return 0.0 };
    let lang = c.lang;
    let mut cy = y;

    // Plan and price.
    let plan = find_plan(st, &inv.plan_id);
    let title = plan.map_or_else(|| inv.plan_id.clone(), |p| plan_name(p, lang));
    let strong = c.strong().to_string();
    let muted = c.muted().to_string();
    c.label(&title, x, cy, FONT_HEADING, &strong);
    if let Some(p) = plan {
        if inv.charge_usd_micros.is_none() {
            c.text(&format!("${}", p.price_usd), x + w, cy, FONT_HEADING, &strong, TextAlign::Right, TextBaseline::Top);
        }
    }
    cy += 22.0;
    if let Some(charge) = inv.charge_usd_micros {
        let line = fill(c.t(CheckoutKey::UpgradeCharge), &[("amount", &format_usd_micros(charge))]);
        c.label(&line, x, cy, FONT_SMALL_STRONG, &strong);
        cy += 18.0;
    }
    let hint = c.wrap(c.t(CheckoutKey::PayHint), FONT_SMALL, w);
    for (i, line) in hint.iter().enumerate() {
        c.label(line, x, cy + i as f64 * 14.0, FONT_SMALL, &muted);
    }
    cy += hint.len() as f64 * 14.0 + 10.0;

    // Chips, exactly as `pay[]` returned them.
    cy += draw_chips(c, &inv.pay, inv.selected, x, cy, w) + 6.0;

    if inv.phase == InvoicePhase::Expired {
        cy += draw_expired(c, x, cy, w) + 8.0;
    } else if let Some(option) = inv.pay.get(inv.selected) {
        cy += draw_option(c, st, option, x, cy, w) + 12.0;
    }

    // Anonymous accounts keep the key one click away while the payment is open.
    if let Some(anon) = st.anon.as_ref() {
        c.label(&c.t(CheckoutKey::KeyHeading).to_uppercase(), x, cy, FONT_SMALL, &mlc_core::apply_opacity(&c.tt.item_text, 0.75));
        cy += 16.0;
        cy += draw_key_row(c, anon, st.is_copied(CopyTarget::Key), x, cy, w) + 12.0;
    }

    // Status and the server's notes.
    cy += draw_status(c, inv.phase, &inv.notes, x, cy, w);

    // Footer.
    cy += 6.0;
    if st.anon.is_none() {
        c.button("back", WidgetRect::new(x, cy - 2.0, 90.0, CTRL_H), c.t(CheckoutKey::Back), Btn::Secondary, true);
    }
    c.link("have_code", c.t(CheckoutKey::HaveCode), x, cy + 4.0, Some(x + w));
    cy += CTRL_H;
    if let Some(error) = &st.error {
        cy += 8.0;
        cy += error_lines(c, error, x, cy, w);
    }
    cy - y
}

fn draw_chips(c: &mut Canvas<'_>, pay: &[PayOptionUi], selected: usize, x: f64, y: f64, w: f64) -> f64 {
    let mut cx = x;
    let mut cy = y;
    for (i, option) in pay.iter().enumerate() {
        let label = format!("{} \u{00b7} {}", option.asset, option.chain);
        let chip_w = (c.width_of(&label, FONT_SMALL_STRONG) + 24.0).max(64.0).min(w);
        if cx + chip_w > x + w + 0.5 && cx > x {
            cx = x;
            cy += CHIP_H + CHIP_GAP;
        }
        if c.drawing() {
            let rect = WidgetRect::new(cx, cy, chip_w, CHIP_H);
            let id = format!("chip:{i}");
            let hovered = c.hovered(&id);
            let is_selected = i == selected;
            let (bg, fg, border) = if is_selected {
                (c.theme.accent.clone(), c.theme.accent_text.clone(), c.theme.accent.clone())
            } else if hovered {
                (c.tt.button_bg_hover.clone(), c.tt.item_text.clone(), c.tt.separator.clone())
            } else {
                (c.tt.dropdown_bg.clone(), c.tt.item_text.clone(), c.tt.separator.clone())
            };
            c.ctx.set_fill_color(&bg);
            c.ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, CHIP_H / 2.0);
            c.ctx.set_stroke_color(&border);
            c.ctx.set_stroke_width(1.0);
            c.ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, CHIP_H / 2.0);
            c.ctx.set_font(FONT_SMALL_STRONG);
            let fitted = fit_text_to_width(&*c.ctx, &label, chip_w - 12.0);
            c.text(&fitted, rect.center_x(), rect.center_y(), FONT_SMALL_STRONG, &fg, TextAlign::Center, TextBaseline::Middle);
            c.register(&id, rect);
        }
        cx += chip_w + CHIP_GAP;
    }
    if pay.is_empty() {
        return 0.0;
    }
    (cy - y) + CHIP_H
}

fn draw_expired(c: &mut Canvas<'_>, x: f64, y: f64, w: f64) -> f64 {
    let danger = c.tt.danger.clone();
    c.label(c.t(CheckoutKey::InvoiceExpired), x, y, FONT_BODY, &danger);
    let rect = WidgetRect::new(x, y + 26.0, 150.0, BTN_H);
    c.button("new_invoice", rect, c.t(CheckoutKey::NewInvoice), Btn::Primary, true);
    let _ = w;
    26.0 + BTN_H
}

/// QR on the left; amount, address and countdown on the right.
fn draw_option(c: &mut Canvas<'_>, st: &CheckoutState, option: &PayOptionUi, x: f64, y: f64, w: f64) -> f64 {
    let muted = c.muted().to_string();
    let strong = c.strong().to_string();
    let label_color = mlc_core::apply_opacity(&c.tt.item_text, 0.75);

    // QR: dark on light, integer module pixels.
    if c.drawing() {
        let qr_rect = WidgetRect::new(x, y, QR_SIDE, QR_SIDE);
        match option.qr.as_ref() {
            Some(qr) => draw_qr(c, qr, qr_rect),
            None => {
                let bg = c.tt.dropdown_bg.clone();
                let border = c.tt.separator.clone();
                c.rounded_box(qr_rect, &bg, &border);
                let lines = c.wrap(c.t(CheckoutKey::QrUnavailable), FONT_SMALL, QR_SIDE - 20.0);
                for (i, line) in lines.iter().enumerate() {
                    c.text(line, qr_rect.center_x(), qr_rect.center_y() + (i as f64 - (lines.len() as f64 - 1.0) / 2.0) * 14.0, FONT_SMALL, &muted, TextAlign::Center, TextBaseline::Middle);
                }
            }
        }
    }

    let col_x = x + QR_SIDE + 16.0;
    let col_w = w - QR_SIDE - 16.0;
    let mut cy = y;

    // Amount.
    c.label(c.t(CheckoutKey::Amount), col_x, cy, FONT_SMALL, &label_color);
    cy += 16.0;
    let amount_text = format!("{} {}", option.amount, option.asset);
    c.ctx.set_font(FONT_HEADING);
    let amount_fit = fit_text_to_width(&*c.ctx, &amount_text, col_w - SMALL_BTN_W - 8.0);
    c.text(&amount_fit, col_x, cy + SMALL_BTN_H / 2.0, FONT_HEADING, &strong, TextAlign::Left, TextBaseline::Middle);
    let copied_amount = st.is_copied(CopyTarget::Amount);
    let amount_btn = WidgetRect::new(col_x + col_w - SMALL_BTN_W, cy, SMALL_BTN_W, SMALL_BTN_H);
    c.small_button("copy_amount", amount_btn, if copied_amount { c.t(CheckoutKey::Copied) } else { c.t(CheckoutKey::Copy) }, copied_amount);
    cy += SMALL_BTN_H + 14.0;

    // Address.
    c.label(c.t(CheckoutKey::Address), col_x, cy, FONT_SMALL, &label_color);
    cy += 16.0;
    let lines = c.wrap_chars(&option.address, FONT_MONO, col_w, 3);
    for (i, line) in lines.iter().enumerate() {
        c.label(line, col_x, cy + i as f64 * 16.0, FONT_MONO, &strong);
    }
    cy += lines.len() as f64 * 16.0 + 6.0;
    let copied_address = st.is_copied(CopyTarget::Address);
    let address_btn = WidgetRect::new(col_x, cy, SMALL_BTN_W, SMALL_BTN_H);
    c.small_button("copy_address", address_btn, if copied_address { c.t(CheckoutKey::Copied) } else { c.t(CheckoutKey::Copy) }, copied_address);
    cy += SMALL_BTN_H;

    // Countdown pinned to the bottom of the QR square.
    if let Some(secs) = st.remaining_secs() {
        let text = fill(c.t(CheckoutKey::ExpiresIn), &[("time", &format_countdown(secs))]);
        let color = if secs < 60 { c.tt.warning.clone() } else { muted.clone() };
        let ty = (y + QR_SIDE - 14.0).max(cy + 6.0);
        c.label(&text, col_x, ty, FONT_SMALL, &color);
        cy = cy.max(ty + 14.0);
    }
    (cy - y).max(QR_SIDE)
}

/// The processor's QR, black on white, in whole-pixel modules.
fn draw_qr(c: &mut Canvas<'_>, qr: &QrMatrix, rect: WidgetRect) {
    c.ctx.set_fill_color(QR_LIGHT);
    c.ctx.fill_rect(rect.x, rect.y, rect.width, rect.height);
    // Whole DEVICE pixels per module, so the modules stay crisp at any display
    // scale (a phone camera reads the edges); positions in CSS pixels.
    let dpr = c.ctx.dpr().max(1.0);
    let module_dev = qr.module_px((rect.width.min(rect.height) * dpr).floor());
    let span_dev = module_dev * f64::from(qr.size);
    let ox = ((rect.x * dpr + (rect.width * dpr - span_dev) / 2.0).round()) / dpr;
    let oy = ((rect.y * dpr + (rect.height * dpr - span_dev) / 2.0).round()) / dpr;
    let module = module_dev / dpr;
    c.ctx.set_fill_color(QR_DARK);
    for r in &qr.dark {
        c.ctx.fill_rect(
            ox + f64::from(r.x) * module,
            oy + f64::from(r.y) * module,
            f64::from(r.w) * module,
            f64::from(r.h) * module,
        );
    }
    c.ctx.set_stroke_color(&c.tt.separator);
    c.ctx.set_stroke_width(1.0);
    c.ctx.stroke_rect(rect.x, rect.y, rect.width, rect.height);
}

/// The English-or-translated line for one integrity note.
pub(crate) fn note_text(note: &InvoiceNote, lang: Language) -> String {
    match note {
        InvoiceNote::Partial(a) => fill(
            CheckoutKey::NotePartial.get(lang),
            &[("received", &a.received), ("expected", &a.expected), ("shortfall", &a.shortfall), ("asset", &a.asset)],
        ),
        InvoiceNote::Over(a) => fill(
            CheckoutKey::NoteOver.get(lang),
            &[("received", &a.received), ("expected", &a.expected), ("asset", &a.asset)],
        ),
        InvoiceNote::Late => CheckoutKey::NoteLate.get(lang).to_string(),
        InvoiceNote::Failed => CheckoutKey::NoteFailed.get(lang).to_string(),
        InvoiceNote::FundsPending => CheckoutKey::NoteFundsPending.get(lang).to_string(),
        InvoiceNote::Reverted => CheckoutKey::NoteReverted.get(lang).to_string(),
        InvoiceNote::Refunded => CheckoutKey::NoteRefunded.get(lang).to_string(),
    }
}

fn draw_status(c: &mut Canvas<'_>, phase: InvoicePhase, notes: &[InvoiceNote], x: f64, y: f64, w: f64) -> f64 {
    let mut cy = y;
    let status = match phase {
        InvoicePhase::Waiting => Some((CheckoutKey::Waiting, c.tt.accent.clone())),
        InvoicePhase::Processing => Some((CheckoutKey::Processing, c.tt.success.clone())),
        InvoicePhase::Paid | InvoicePhase::Expired | InvoicePhase::Failed => None,
    };
    if let Some((key, dot)) = status {
        if c.drawing() {
            c.ctx.set_fill_color(&dot);
            c.ctx.begin_path();
            c.ctx.arc(x + 4.0, cy + 8.0, 3.5, 0.0, std::f64::consts::TAU);
            c.ctx.fill();
        }
        let color = c.strong().to_string();
        c.label(c.t(key), x + 16.0, cy, FONT_BODY, &color);
        cy += 22.0;
    }
    let warn = c.tt.warning.clone();
    for note in notes {
        let text = note_text(note, c.lang);
        let lines = c.wrap(&text, FONT_SMALL, w);
        for (i, line) in lines.iter().enumerate() {
            c.label(line, x, cy + i as f64 * 15.0, FONT_SMALL, &warn);
        }
        cy += lines.len() as f64 * 15.0 + 4.0;
    }
    cy - y
}

// ---- paid / scheduled -------------------------------------------------------

fn success_badge(c: &mut Canvas<'_>, cx: f64, cy: f64) {
    if !c.drawing() {
        return;
    }
    let success = c.tt.success.clone();
    c.ctx.set_stroke_color(&success);
    c.ctx.set_stroke_width(2.0);
    c.ctx.begin_path();
    c.ctx.arc(cx, cy, 24.0, 0.0, std::f64::consts::TAU);
    c.ctx.stroke();
    c.check_mark(cx - 14.0, cy - 14.0, 28.0, &success, 3.0);
}

fn centered_lines(c: &mut Canvas<'_>, text: &str, font: &str, color: &str, x: f64, y: f64, w: f64, step: f64) -> f64 {
    let lines = c.wrap(text, font, w);
    for (i, line) in lines.iter().enumerate() {
        c.text(line, x + w / 2.0, y + i as f64 * step, font, color, TextAlign::Center, TextBaseline::Top);
    }
    lines.len() as f64 * step
}

fn draw_paid(c: &mut Canvas<'_>, st: &CheckoutState, x: f64, y: f64, w: f64) -> f64 {
    let mut cy = y + 12.0;
    success_badge(c, x + w / 2.0, cy + 24.0);
    cy += 64.0;
    let strong = c.strong().to_string();
    let muted = c.muted().to_string();
    cy += centered_lines(c, c.t(CheckoutKey::PaidTitle), FONT_HEADING, &strong, x, cy, w, 22.0) + 2.0;
    cy += centered_lines(c, c.t(CheckoutKey::PaidBody), FONT_SMALL, &muted, x, cy, w, 15.0) + 8.0;

    if let Some(inv) = st.invoice.as_ref() {
        let warn = c.tt.warning.clone();
        for note in &inv.notes {
            let text = note_text(note, c.lang);
            cy += centered_lines(c, &text, FONT_SMALL, &warn, x, cy, w, 15.0) + 4.0;
        }
    }

    if let Some(anon) = st.anon.as_ref() {
        cy += 8.0;
        match (anon.auto_sign_in, anon.sign_in) {
            (true, SignInPhase::Idle | SignInPhase::Working) => {
                cy += centered_lines(c, c.t(CheckoutKey::SigningIn), FONT_SMALL, &muted, x, cy, w, 15.0) + 4.0;
            }
            (true, SignInPhase::Done) => {
                let success = c.tt.success.clone();
                cy += centered_lines(c, c.t(CheckoutKey::SignedInWithKey), FONT_SMALL, &success, x, cy, w, 15.0) + 4.0;
            }
            (true, SignInPhase::Failed) => {
                if let Some(error) = &st.error {
                    cy += error_lines(c, error, x, cy, w) + 8.0;
                }
                c.button("retry", WidgetRect::new(x, cy, w, BTN_H), c.t(CheckoutKey::Retry), Btn::Primary, true);
                cy += BTN_H + 8.0;
            }
            (false, _) => {
                cy += centered_lines(c, c.t(CheckoutKey::KeyUseLater), FONT_SMALL, &muted, x, cy, w, 15.0) + 8.0;
            }
        }
        // The key stays copyable until the modal is done with it.
        let needs_key = !(anon.auto_sign_in && anon.sign_in == SignInPhase::Done);
        if needs_key {
            cy += draw_key_row(c, anon, st.is_copied(CopyTarget::Key), x, cy, w) + 12.0;
            c.button("done", WidgetRect::new(x, cy, w, BTN_H), c.t(CheckoutKey::Close), Btn::Secondary, true);
            cy += BTN_H;
        }
    }
    cy - y + 8.0
}

fn draw_scheduled(c: &mut Canvas<'_>, st: &CheckoutState, x: f64, y: f64, w: f64) -> f64 {
    let mut cy = y + 12.0;
    success_badge(c, x + w / 2.0, cy + 24.0);
    cy += 64.0;
    let strong = c.strong().to_string();
    let date = st
        .scheduled_on
        .as_deref()
        .map(|d| d.chars().take(10).collect::<String>())
        .unwrap_or_default();
    let text = fill(c.t(CheckoutKey::DowngradeScheduled), &[("date", &date)]);
    cy += centered_lines(c, &text, FONT_BODY, &strong, x, cy, w, 18.0) + 16.0;
    c.button("done", WidgetRect::new(x, cy, w, BTN_H), c.t(CheckoutKey::Close), Btn::Secondary, true);
    cy += BTN_H;
    cy - y + 8.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::checkout::{
        AmountUi, CheckoutEvent, HoldingsUi, InvoiceReading, PayOptionUi as Pay, PlanKind as Kind,
    };

    // ---- a recording, headless RenderContext -------------------------------

    #[derive(Default)]
    struct Rec {
        texts: Vec<String>,
        fills: Vec<(f64, f64, f64, f64, String)>,
        fill_color: String,
        /// Device pixel ratio the fake display reports (0 reads as 1).
        scale: f64,
    }

    impl uzor::render::Painter for Rec {
        fn save(&mut self) {}
        fn restore(&mut self) {}
        fn translate(&mut self, _x: f64, _y: f64) {}
        fn rotate(&mut self, _angle: f64) {}
        fn scale(&mut self, _x: f64, _y: f64) {}
        fn set_fill_color(&mut self, color: &str) {
            self.fill_color = color.to_string();
        }
        fn set_global_alpha(&mut self, _alpha: f64) {}
        fn set_stroke_color(&mut self, _color: &str) {}
        fn set_stroke_width(&mut self, _width: f64) {}
        fn set_line_dash(&mut self, _pattern: &[f64]) {}
        fn set_line_cap(&mut self, _cap: &str) {}
        fn set_line_join(&mut self, _join: &str) {}
        fn begin_path(&mut self) {}
        fn move_to(&mut self, _x: f64, _y: f64) {}
        fn line_to(&mut self, _x: f64, _y: f64) {}
        fn close_path(&mut self) {}
        fn rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
        fn arc(&mut self, _cx: f64, _cy: f64, _r: f64, _s: f64, _e: f64) {}
        fn ellipse(&mut self, _cx: f64, _cy: f64, _rx: f64, _ry: f64, _rot: f64, _s: f64, _e: f64) {}
        fn quadratic_curve_to(&mut self, _cpx: f64, _cpy: f64, _x: f64, _y: f64) {}
        fn bezier_curve_to(&mut self, _a: f64, _b: f64, _c: f64, _d: f64, _x: f64, _y: f64) {}
        fn stroke(&mut self) {}
        fn fill(&mut self) {}
    }
    impl uzor::render::TextRenderer for Rec {
        fn set_font(&mut self, _font: &str) {}
        fn set_text_align(&mut self, _align: uzor::render::TextAlign) {}
        fn set_text_baseline(&mut self, _baseline: uzor::render::TextBaseline) {}
        fn fill_text(&mut self, text: &str, _x: f64, _y: f64) {
            self.texts.push(text.to_string());
        }
        fn stroke_text(&mut self, _text: &str, _x: f64, _y: f64) {}
    }
    impl uzor::render::TextMetrics for Rec {
        fn measure_text(&self, text: &str) -> f64 {
            text.chars().count() as f64 * 6.5
        }
        fn text_bounds(&self, _text: &str, _font: &str) -> uzor::render::TextBounds {
            uzor::render::TextBounds { x: 0.0, y: 0.0, w: 0.0, h: 0.0, ascent: 0.0, descent: 0.0 }
        }
    }
    impl uzor::render::Masking for Rec {
        fn clip(&mut self) {}
    }
    impl uzor::render::Effects for Rec {}
    impl uzor::render::ShapeHelpers for Rec {
        fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
            self.fills.push((x, y, w, h, self.fill_color.clone()));
        }
        fn stroke_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
    }
    impl uzor::render::GradientPainter for Rec {}
    impl uzor::render::UiEffectHelpers for Rec {}
    impl uzor::render::BatchPainter for Rec {}
    impl uzor::render::RenderContext for Rec {
        fn dpr(&self) -> f64 {
            if self.scale > 0.0 {
                self.scale
            } else {
                1.0
            }
        }
    }
    impl RenderContext for Rec {
        fn chart_width(&self) -> f64 {
            1000.0
        }
        fn chart_height(&self) -> f64 {
            600.0
        }
        fn bar_to_x(&self, bar: f64) -> f64 {
            bar
        }
        fn price_to_y(&self, price: f64) -> f64 {
            price
        }
        fn bars(&self) -> &[mlc_core::Bar] {
            &[]
        }
        fn set_coordinate_space(&mut self, _w: f64, _h: f64, _s: f64, _b: f64, _lo: f64, _hi: f64) {}
    }

    const NOW: u64 = 1_800_000_000_000;

    fn render(st: &CheckoutState) -> (Rec, CheckoutResult) {
        render_at(st, 1.0)
    }

    fn render_at(st: &CheckoutState, scale: f64) -> (Rec, CheckoutResult) {
        let mut ctx = Rec { scale, ..Rec::default() };
        let mut coord = InputCoordinator::new();
        let result = render_checkout_modal(
            &mut ctx,
            1280.0,
            900.0,
            st,
            &FrameTheme::default(),
            &ToolbarTheme::default(),
            0,
            &mut coord,
        );
        (ctx, result)
    }

    fn has_hit(r: &CheckoutResult, id: &str) -> bool {
        r.hits.iter().any(|(k, _)| k == id)
    }

    fn has_text(ctx: &Rec, needle: &str) -> bool {
        ctx.texts.iter().any(|t| t.contains(needle))
    }

    fn plan(id: &str, kind: Kind, price: u32, tier: Option<u8>, quota: Option<u64>) -> PlanUi {
        PlanUi {
            id: id.to_string(),
            kind,
            monthly: kind == Kind::Quant,
            price_usd: price,
            tier,
            quota,
            onion_only: false,
        }
    }

    fn open_with_plans_and_gate(signed_in: bool, anon_purchase: bool) -> CheckoutState {
        let mut st = CheckoutState { anon_purchase, ..CheckoutState::default() };
        st.open(signed_in);
        st.take_cmd(NOW);
        st.apply(
            CheckoutEvent::Plans {
                plans: vec![
                    plan("paid", Kind::PaidAccount, 5, None, None),
                    plan("anon", Kind::AnonAccount, 10, None, None),
                    plan("q20", Kind::Quant, 20, Some(1), Some(100)),
                    plan("q50", Kind::Quant, 50, Some(2), None),
                ],
                onion: false,
                enabled: true,
            },
            NOW,
        );
        st
    }

    /// The plans step with the anonymous purchase switched on, so the anonymous
    /// flow stays covered while `ANON_PURCHASE_ENABLED` is off.
    fn open_with_plans(signed_in: bool) -> CheckoutState {
        open_with_plans_and_gate(signed_in, true)
    }

    fn qr_document() -> QrMatrix {
        QrMatrix::parse(concat!(
            r##"<svg viewBox="0 0 5 5"><rect width="5" height="5" fill="#fff"/>"##,
            r##"<path fill="#000" d="M2 1h1v1H2V1M1 2h1v1H1V2M2 2h1v1H2V2M3 2h1v1H3V2M2 3h1v1H2V3"/></svg>"##
        ))
        .expect("qr parses")
    }

    fn sample_pay() -> Vec<Pay> {
        vec![
            Pay {
                chain: "evm:1".into(),
                asset: "USDC".into(),
                address: "0x00000000000000000000000000000000000000aa".into(),
                amount: "20.000000".into(),
                expires_at_ms: Some(NOW as i64 + 540_000),
                qr: Some(qr_document()),
            },
            Pay {
                chain: "tron".into(),
                asset: "USDT".into(),
                address: "TMockAddress000000000000000000000000".into(),
                amount: "20.000000".into(),
                expires_at_ms: Some(NOW as i64 + 540_000),
                qr: None,
            },
        ]
    }

    fn paying(plan_id: &str, key: Option<&str>) -> CheckoutState {
        let mut st = open_with_plans(key.is_none());
        st.apply(
            CheckoutEvent::Started {
                invoice_id: "grp_1".into(),
                plan_id: plan_id.into(),
                pay: sample_pay(),
                account_key: key.map(str::to_string),
                charge_usd_micros: None,
            },
            NOW,
        );
        st.tick(NOW);
        st
    }

    // ---- plans -------------------------------------------------------------

    #[test]
    fn the_plans_step_draws_every_card_with_price_and_period() {
        let st = open_with_plans(true);
        let (ctx, r) = render(&st);
        for text in ["Choose a plan", "Paid account", "Anonymous account", "Quant tier 1", "Quant tier 2", "$5", "$10", "$20", "$50"] {
            assert!(has_text(&ctx, text), "missing {text:?} in {:?}", ctx.texts);
        }
        assert!(has_text(&ctx, "one-time") && has_text(&ctx, "per month"));
        assert!(has_text(&ctx, "Monthly subscription. Quota: 100."), "the operator quota is printed");
        for id in ["plan:paid", "plan:anon", "plan:q20", "plan:q50", "have_code"] {
            assert!(has_hit(&r, id), "missing hit {id}");
        }
        assert!(has_text(&ctx, "Buy") && has_text(&ctx, "Subscribe"));
    }

    #[test]
    fn the_release_gate_keeps_the_anon_card_and_the_key_link_off_the_plans_step() {
        // The state as the shell builds it: the gate constant decides.
        for signed_in in [false, true] {
            let st = open_with_plans_and_gate(signed_in, crate::ui::checkout::ANON_PURCHASE_ENABLED);
            let (ctx, r) = render(&st);
            assert!(!has_hit(&r, "plan:anon"), "no anonymous card (signed_in={signed_in})");
            assert!(!has_text(&ctx, "Anonymous account"), "{:?}", ctx.texts);
            assert!(!has_hit(&r, "have_key"), "no account-key entry (signed_in={signed_in})");
            for id in ["plan:paid", "plan:q20", "plan:q50", "have_code"] {
                assert!(has_hit(&r, id), "missing hit {id}");
            }
        }
    }

    #[test]
    fn owned_plans_read_as_owned_and_take_no_click() {
        let mut st = open_with_plans(true);
        st.apply(
            CheckoutEvent::Holdings(HoldingsUi { lifetime_paid: true, quant_plan_id: Some("q20".into()), ..HoldingsUi::default() }),
            NOW,
        );
        let (ctx, r) = render(&st);
        assert!(has_text(&ctx, "\u{2713} Owned"), "{:?}", ctx.texts);
        assert!(has_text(&ctx, "\u{2713} Current plan"));
        assert!(has_text(&ctx, "Upgrade"), "the dearer tier offers an upgrade");
        assert!(!has_hit(&r, "plan:paid"), "an owned plan is not clickable");
        assert!(!has_hit(&r, "plan:q20"), "the running tier is not clickable");
        assert!(has_hit(&r, "plan:q50"));
        assert!(has_hit(&r, "plan:anon"), "a paid account can still buy an anonymous one");
    }

    #[test]
    fn a_busy_card_takes_no_second_click() {
        let mut st = open_with_plans(true);
        st.click("plan:paid", NOW);
        let (ctx, r) = render(&st);
        assert!(has_text(&ctx, "\u{2026}"), "the starting card shows a pending mark");
        assert!(!r.hits.iter().any(|(k, _)| k.starts_with("plan:")), "no card is clickable while one is starting");
    }

    #[test]
    fn a_loading_list_says_so_and_a_failed_one_offers_retry() {
        let mut st = CheckoutState::default();
        st.open(true);
        let (ctx, _) = render(&st);
        assert!(has_text(&ctx, "Loading plans"));

        st.take_cmd(NOW);
        st.apply(
            CheckoutEvent::Failed { op: crate::ui::checkout::CheckoutOp::LoadPlans, error: CheckoutError::Network },
            NOW,
        );
        let (ctx, r) = render(&st);
        assert!(has_text(&ctx, "Network error. Check your connection and try again."));
        assert!(has_hit(&r, "retry"));
    }

    #[test]
    fn a_wire_error_prints_its_user_facing_sentence() {
        let mut st = open_with_plans(true);
        st.click("plan:paid", NOW);
        st.take_cmd(NOW);
        st.apply(
            CheckoutEvent::Failed {
                op: crate::ui::checkout::CheckoutOp::Start,
                error: CheckoutError::from_wire(409, "already_owned", None),
            },
            NOW,
        );
        let (ctx, _) = render(&st);
        assert!(has_text(&ctx, "You already own this plan."), "{:?}", ctx.texts);
    }

    // ---- pay ---------------------------------------------------------------

    #[test]
    fn the_pay_step_draws_chips_amount_address_qr_and_countdown_from_pay() {
        let st = paying("paid", None);
        let (ctx, r) = render(&st);
        for text in [
            "USDC \u{00b7} evm:1",
            "USDT \u{00b7} tron",
            "20.000000 USDC",
            "Amount",
            "Address",
            "Expires in 09:00",
            "Waiting for payment",
        ] {
            assert!(has_text(&ctx, text), "missing {text:?} in {:?}", ctx.texts);
        }
        // The address is wrapped by characters; every character is on screen.
        assert!(
            ctx.texts.concat().contains("0x00000000000000000000000000000000000000aa"),
            "{:?}",
            ctx.texts
        );
        for id in ["chip:0", "chip:1", "copy_amount", "copy_address", "have_code", "back"] {
            assert!(has_hit(&r, id), "missing hit {id}");
        }
        assert!(!has_hit(&r, "copy_key"), "only an anonymous purchase has a key row");
    }

    #[test]
    fn the_qr_is_drawn_black_on_white_in_whole_pixel_modules() {
        let st = paying("paid", None);
        let (ctx, _) = render(&st);
        let white: Vec<_> = ctx.fills.iter().filter(|f| f.4 == QR_LIGHT).collect();
        let black: Vec<_> = ctx.fills.iter().filter(|f| f.4 == QR_DARK).collect();
        assert_eq!(white.len(), 1, "one white square behind the modules");
        assert_eq!((white[0].2, white[0].3), (QR_SIDE, QR_SIDE));
        assert_eq!(black.len(), 3, "three merged row runs for the plus-shaped test code");
        let module = 30.0; // floor(152 / 5)
        for f in &black {
            assert_eq!(f.2 % module, 0.0);
            assert_eq!(f.3, module);
            assert_eq!(f.0.fract(), 0.0, "modules sit on whole pixels");
            assert_eq!(f.1.fract(), 0.0);
        }
    }

    #[test]
    fn at_a_fractional_display_scale_the_modules_still_land_on_whole_device_pixels() {
        let st = paying("paid", None);
        let (ctx, _) = render_at(&st, 1.25);
        let black: Vec<_> = ctx.fills.iter().filter(|f| f.4 == QR_DARK).collect();
        assert_eq!(black.len(), 3);
        let on_device_grid = |v: f64| ((v * 1.25) - (v * 1.25).round()).abs() < 1e-6;
        for f in &black {
            assert!(on_device_grid(f.0) && on_device_grid(f.1), "origin {:?} is off the device grid", (f.0, f.1));
            assert!(on_device_grid(f.2) && on_device_grid(f.3), "size {:?} is off the device grid", (f.2, f.3));
        }
    }

    #[test]
    fn an_option_without_a_readable_qr_says_so_and_keeps_the_address() {
        let mut st = paying("paid", None);
        st.click("chip:1", NOW);
        let (ctx, _) = render(&st);
        // The sentence wraps inside the QR square; both halves are on screen.
        assert!(has_text(&ctx, "QR code unavailable"));
        assert!(has_text(&ctx, "Use the address"));
        assert!(has_text(&ctx, "TMockAddress"));
        assert!(!ctx.fills.iter().any(|f| f.4 == QR_DARK));
    }

    #[test]
    fn the_countdown_counts_down_and_never_goes_negative() {
        let mut st = paying("paid", None);
        st.tick(NOW + 500_000);
        let (ctx, _) = render(&st);
        assert!(has_text(&ctx, "Expires in 00:40"));
        st.tick(NOW + 900_000);
        let (ctx, _) = render(&st);
        assert!(has_text(&ctx, "Expires in 00:00"));
    }

    #[test]
    fn an_expired_invoice_replaces_the_payment_detail_with_a_new_invoice_button() {
        let mut st = paying("paid", None);
        st.take_cmd(NOW + 3_000);
        st.apply(
            CheckoutEvent::Invoice(InvoiceReading {
                invoice_id: "grp_1".into(),
                status: "new".into(),
                live_status: Some("expired".into()),
                grant_state: "none".into(),
                ..InvoiceReading::default()
            }),
            NOW + 4_000,
        );
        let (ctx, r) = render(&st);
        assert!(has_text(&ctx, "This invoice has expired."));
        assert!(has_hit(&r, "new_invoice"));
        assert!(!has_hit(&r, "copy_address"), "an expired address is not offered");
        assert!(has_hit(&r, "chip:0"), "the chips stay as the server returned them");
    }

    #[test]
    fn partial_over_and_late_notes_print_the_servers_amounts() {
        let mut st = paying("paid", None);
        st.take_cmd(NOW + 3_000);
        st.apply(
            CheckoutEvent::Invoice(InvoiceReading {
                invoice_id: "grp_1".into(),
                status: "new".into(),
                live_status: Some("processing".into()),
                grant_state: "none".into(),
                flags: vec!["partial".into(), "late".into()],
                partial: Some(AmountUi {
                    asset: "USDT".into(),
                    received: "2.000000".into(),
                    expected: "5.000000".into(),
                    shortfall: "3.000000".into(),
                }),
                ..InvoiceReading::default()
            }),
            NOW + 4_000,
        );
        let (ctx, _) = render(&st);
        assert!(has_text(&ctx, "Payment detected"));
        let all = ctx.texts.join(" ");
        assert!(all.contains("Received 2.000000 of 5.000000 USDT"), "{all}");
        assert!(all.contains("remaining 3.000000 USDT"), "{all}");
        assert!(all.contains("The payment arrived after the invoice expired."), "{all}");
    }

    #[test]
    fn an_upgrade_prints_its_charge() {
        let mut st = open_with_plans(true);
        st.apply(
            CheckoutEvent::Started {
                invoice_id: "grp_9".into(),
                plan_id: "q50".into(),
                pay: sample_pay(),
                account_key: None,
                charge_usd_micros: Some(12_000_000),
            },
            NOW,
        );
        let (ctx, _) = render(&st);
        assert!(has_text(&ctx, "Upgrade charge: $12.00"));
    }

    // ---- anonymous key gate ------------------------------------------------

    #[test]
    fn the_key_gate_shows_the_key_and_keeps_continue_dead_until_saved() {
        let mut st = paying("anon", Some("0F4Z-AAAA-BBBB-CCCC"));
        assert_eq!(st.step, CheckoutStep::KeyGate);
        let (ctx, r) = render(&st);
        assert!(has_text(&ctx, "Save your account key"));
        assert!(has_text(&ctx, "0F4Z-AAAA-BBBB-CCCC"));
        assert!(has_hit(&r, "copy_key") && has_hit(&r, "key_saved") && has_hit(&r, "back"));
        assert!(!has_hit(&r, "key_continue"), "continue is dead before the box is ticked");
        assert!(!has_text(&ctx, "20.000000"), "no payment detail behind the gate");

        st.click("key_saved", NOW);
        let (_, r) = render(&st);
        assert!(has_hit(&r, "key_continue"));

        st.click("key_continue", NOW);
        let (ctx, r) = render(&st);
        assert!(has_text(&ctx, "20.000000 USDC"));
        assert!(has_hit(&r, "copy_key"), "the key stays one click away while paying");
        assert!(!has_hit(&r, "back"), "no way back past the gate once paying");
    }

    // ---- success -----------------------------------------------------------

    #[test]
    fn a_paid_signed_out_anonymous_purchase_shows_success_and_signing_in() {
        let mut st = paying("anon", Some("KEY-1"));
        st.click("key_saved", NOW);
        st.click("key_continue", NOW);
        st.apply(
            CheckoutEvent::Invoice(InvoiceReading {
                invoice_id: "grp_1".into(),
                status: "settled".into(),
                live_status: Some("settled".into()),
                grant_state: "granted".into(),
                ..InvoiceReading::default()
            }),
            NOW + 3_100,
        );
        let (ctx, _) = render(&st);
        assert!(has_text(&ctx, "Payment received"));
        assert!(has_text(&ctx, "Your account is updated."));
        assert!(has_text(&ctx, "Signing you in with your key"));
    }

    #[test]
    fn a_signed_in_buyer_of_an_anonymous_account_gets_the_key_and_a_close_button() {
        let mut st = paying("anon", None);
        // Re-run as a signed-in buyer of the anonymous plan.
        st.apply(
            CheckoutEvent::Started {
                invoice_id: "grp_2".into(),
                plan_id: "anon".into(),
                pay: sample_pay(),
                account_key: Some("KEY-2".into()),
                charge_usd_micros: None,
            },
            NOW,
        );
        st.click("key_saved", NOW);
        st.click("key_continue", NOW);
        st.apply(
            CheckoutEvent::Invoice(InvoiceReading {
                invoice_id: "grp_2".into(),
                status: "settled".into(),
                grant_state: "granted".into(),
                ..InvoiceReading::default()
            }),
            NOW + 3_100,
        );
        let (ctx, r) = render(&st);
        assert!(has_text(&ctx, "Sign out and sign in with this key to use the new account."));
        assert!(has_text(&ctx, "KEY-2"));
        assert!(has_hit(&r, "copy_key") && has_hit(&r, "done"));
    }

    #[test]
    fn a_queued_downgrade_prints_its_date() {
        let mut st = open_with_plans(true);
        st.apply(CheckoutEvent::ChangeScheduled { effective: Some("2026-10-25T00:00:00+00:00".into()) }, NOW);
        let (ctx, r) = render(&st);
        assert!(has_text(&ctx, "Downgrade scheduled for 2026-10-25."), "{:?}", ctx.texts);
        assert!(has_hit(&r, "done"));
    }

    // ---- key entry ---------------------------------------------------------

    #[test]
    fn the_key_entry_page_shows_a_field_and_a_sign_in_button() {
        let mut st = open_with_plans(false);
        st.click("have_key", NOW);
        let (ctx, r) = render(&st);
        assert!(has_text(&ctx, "Sign in with your account key"));
        assert!(has_text(&ctx, "Sign in"));
        for id in ["key_input", "back", "key_submit"] {
            assert!(has_hit(&r, id), "missing hit {id}");
        }
        assert!(r.input_rect.width > 0.0, "the input side gets the field's rect for the caret");

        st.submit_key("KEY");
        let (_, r) = render(&st);
        assert!(!has_hit(&r, "key_submit"), "the button is dead while a sign-in is in flight");
    }

    #[test]
    fn a_refused_key_prints_the_sentence_under_the_field() {
        let mut st = open_with_plans(false);
        st.click("have_key", NOW);
        st.submit_key("WRONG");
        st.take_cmd(NOW);
        st.apply(
            CheckoutEvent::Failed { op: crate::ui::checkout::CheckoutOp::SignIn, error: CheckoutError::InvalidKey },
            NOW,
        );
        let (ctx, r) = render(&st);
        assert!(has_text(&ctx, "The account key was not accepted."), "{:?}", ctx.texts);
        assert!(has_hit(&r, "key_submit"), "and the button is live again");
    }

    #[test]
    fn the_plans_step_offers_the_key_link_only_while_signed_out() {
        let (_, r) = render(&open_with_plans(false));
        assert!(has_hit(&r, "have_key"));
        let (_, r) = render(&open_with_plans(true));
        assert!(!has_hit(&r, "have_key"));
    }

    // ---- layout ------------------------------------------------------------

    #[test]
    fn every_step_fits_inside_the_screen_it_is_centred_on() {
        let mut states = vec![open_with_plans(true), paying("paid", None), paying("anon", Some("KEY-3"))];
        let mut scheduled = open_with_plans(true);
        scheduled.apply(CheckoutEvent::ChangeScheduled { effective: None }, NOW);
        states.push(scheduled);
        let mut key_entry = open_with_plans(false);
        key_entry.click("have_key", NOW);
        states.push(key_entry);
        for st in &states {
            let (_, r) = render(st);
            let f = r.modal_rect;
            assert!(f.x >= 0.0 && f.y >= 0.0 && f.right() <= 1280.0 && f.bottom() <= 900.0, "{:?} {:?}", st.step, f);
            assert_eq!(f.width, MODAL_W);
            for (id, rect) in &r.hits {
                assert!(
                    rect.x >= f.x && rect.right() <= f.right() + 0.5 && rect.y >= f.y && rect.bottom() <= f.bottom() + 0.5,
                    "{id} {rect:?} escapes the frame {f:?} on {:?}",
                    st.step
                );
            }
        }
    }

    #[test]
    fn note_text_fills_every_placeholder_in_every_language() {
        let amounts = AmountUi {
            asset: "USDT".into(),
            received: "2.5".into(),
            expected: "5".into(),
            shortfall: "2.5".into(),
        };
        for &lang in Language::all() {
            for note in [InvoiceNote::Partial(amounts.clone()), InvoiceNote::Over(amounts.clone())] {
                let text = note_text(&note, lang);
                assert!(!text.contains('{') && !text.contains('}'), "{lang:?}: {text}");
                assert!(text.contains("2.5") && text.contains("USDT"), "{lang:?}: {text}");
            }
        }
    }
}
