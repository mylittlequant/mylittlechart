//! Cloud sign-in / account overlay (cloud builds only).
//!
//! A standard modal shell (`ui::widgets::modal`) over a dimmed backdrop — the
//! shell owns the frame, header, close button, drag registration and theming;
//! this module draws only the body. Pure presentation: every interactive
//! element registers a `cloud_login:*` widget id and the login state machine
//! itself is driven by the shell's cloud session driver through
//! [`CloudLoginState`]'s phase field and one-shot mailboxes.
//!
//! Two sign-in paths, both inside the modal:
//! - OAuth provider buttons (GitHub / Discord / Google) — open the provider
//!   authorize URL carrying our device code; the server confirms the code
//!   after the OAuth dance and the poll completes on its own.
//! - Manual fallback — show the code, user enters it on /dashboard.
//!
//! Gold #F4CD63 stays the one action accent (brandbook,
//! `zeng3ld-landing/web/mylittlechart/README.md`); every other colour comes
//! from the shared modal theme, so the dialog follows the active theme like
//! any other.

use mlc_core::engine::render::{RenderContext, TextAlign, TextBaseline};
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::types::Rect as WidgetRect;
use uzor::WidgetId;

use crate::layout::render_chart::FrameTheme;
use crate::ui::modal_settings::{CloudLoginPhase, CloudLoginState, RedeemStatus};
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::ui::widgets::{
    draw_input, draw_input_cursor, register_dimmer_zones, render_modal_shell, InputConfig,
    ModalFooter, ModalPlacement, ModalShell, ModalShellSpec, ModalTheme, WidgetState,
    SETTINGS_BUTTON_RADIUS,
};
use crate::ui::z_order::ZLayer;

/// The brand's one action accent — the primary CTA fill.
const GOLD: &str = "#f4cd63";
const GOLD_HOVER: &str = "#f7d985";
/// Text on top of the gold CTA.
const GOLD_TEXT: &str = "#0a0f1a";

const MODAL_W: f64 = 430.0;
const BTN_H: f64 = 36.0;
const BTN_GAP: f64 = 10.0;

/// Text-field id for the gift/trial code box in the account view.
///
/// A literal, not a shared constant: this crate cannot depend on `mlc-app`,
/// where `text_input`'s ids live, so every field in every modal here spells its
/// own id (see `chart_browser.rs`'s `"chart_browser_search"`).
/// `mlc_app::text_input::REDEEM_CODE` holds the other half of the pair.
const REDEEM_FIELD: &str = "redeem_code";
/// Height of the code box and the button beside it.
const FIELD_H: f64 = 32.0;
const REDEEM_BTN_W: f64 = 96.0;

/// OAuth providers offered in the modal (id, label).
pub const OAUTH_PROVIDERS: &[(&str, &str)] = &[
    ("github", "Continue with GitHub"),
    ("discord", "Continue with Discord"),
    ("google", "Continue with Google"),
];

/// Geometry the input side needs (header drag).
pub struct CloudLoginResult {
    pub header_rect: WidgetRect,
}

/// Body height per phase — the shell sizes the frame from this.
fn content_height(state: &CloudLoginState) -> f64 {
    match &state.phase {
        // Providers only — no code block, no polling hint.
        CloudLoginPhase::ChooseProvider => {
            24.0 + OAUTH_PROVIDERS.len() as f64 * (BTN_H + BTN_GAP)
        }
        CloudLoginPhase::AwaitingConfirm { .. } => 312.0,
        CloudLoginPhase::SignedIn { .. } => 278.0,
        CloudLoginPhase::Success { .. } => 78.0,
        CloudLoginPhase::Error { .. } => 110.0,
        _ => 52.0,
    }
}

/// Render the cloud sign-in overlay.
pub fn render_cloud_login_modal(
    ctx: &mut dyn RenderContext,
    window_w: f64,
    window_h: f64,
    state: &CloudLoginState,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    current_time_ms: u64,
    input_coordinator: &mut InputCoordinator,
) -> CloudLoginResult {
    let theme = ModalTheme::from_toolbar(
        &frame_theme.modal_bg,
        &toolbar_theme.separator,
        toolbar_theme,
    );
    let title = match &state.phase {
        CloudLoginPhase::SignedIn { .. } => "Account",
        _ => "Sign in",
    };
    let spec = ModalShellSpec {
        // Sign-in must be answered (or dismissed) before anything else.
        dim_backdrop: true,
        // A flow with its own actions in the body has no use for an OK bar.
        footer: ModalFooter::None,
        placement: ModalPlacement::At(state.position),
        layer: ZLayer::ModalOverlay,
        ..ModalShellSpec::new("cloud_login", title, MODAL_W, content_height(state))
    };
    let shell = render_modal_shell(
        ctx,
        input_coordinator,
        &spec,
        &theme,
        (window_w, window_h),
        |ctx, coord, layer_id, shell| {
            register_dimmer_zones(
                coord,
                layer_id,
                "cloud_login",
                shell.frame,
                (window_w, window_h),
            );
            draw_body(
                ctx, coord, layer_id, shell, state, frame_theme, toolbar_theme,
                current_time_ms,
            );
        },
    );
    CloudLoginResult { header_rect: shell.header }
}

#[allow(clippy::too_many_arguments)]
fn draw_body(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    shell: &ModalShell,
    state: &CloudLoginState,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
) {
    let x = shell.content.x;
    let y = shell.content.y;
    let w = shell.content.width;
    let cx = x + w / 2.0;

    match &state.phase {
        CloudLoginPhase::RequestCode | CloudLoginPhase::CreatingCode => {
            ctx.set_font("13px sans-serif");
            ctx.set_fill_color(&tt.item_text_muted);
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("Requesting sign-in code…", cx, y + 26.0);
        }

        // Providers and nothing else. The shell that shows this one signs in
        // through a loopback redirect, so there is no code to display and
        // nothing to poll — the browser comes back to us.
        CloudLoginPhase::ChooseProvider => {
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Top);
            ctx.set_font("13px sans-serif");
            ctx.set_fill_color(&tt.item_text);
            ctx.fill_text("No passwords. Pick a provider —", x, y);
            ctx.fill_text("we only read your handle.", x, y + 20.0);

            let mut btn_y = y + 42.0;
            for (id, label) in OAUTH_PROVIDERS {
                draw_button(
                    ctx,
                    coord,
                    layer_id,
                    &format!("cloud_login:oauth:{}", id),
                    WidgetRect::new(x, btn_y, w, BTN_H),
                    label,
                    BtnStyle::Secondary,
                    tt,
                );
                btn_y += BTN_H + BTN_GAP;
            }
        }

        CloudLoginPhase::AwaitingConfirm { code } => {
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Top);
            ctx.set_font("13px sans-serif");
            ctx.set_fill_color(&tt.item_text);
            ctx.fill_text("No passwords. Pick a provider —", x, y);
            ctx.fill_text("we only read your handle.", x, y + 20.0);

            let mut btn_y = y + 42.0;
            for (id, label) in OAUTH_PROVIDERS {
                draw_button(
                    ctx,
                    coord,
                    layer_id,
                    &format!("cloud_login:oauth:{}", id),
                    WidgetRect::new(x, btn_y, w, BTN_H),
                    label,
                    BtnStyle::Secondary,
                    tt,
                );
                btn_y += BTN_H + BTN_GAP;
            }

            ctx.set_font("10px monospace");
            ctx.set_fill_color(&tt.item_text_muted);
            ctx.set_text_align(TextAlign::Center);
            ctx.fill_text("OR CONFIRM THE CODE ON THE SITE", cx, btn_y + 4.0);

            let code_y = btn_y + 16.0;
            let code_h = 44.0;
            ctx.set_fill_color(&tt.dropdown_bg);
            ctx.fill_rounded_rect(x, code_y, w, code_h, SETTINGS_BUTTON_RADIUS);
            ctx.set_stroke_color(&tt.separator);
            ctx.set_stroke_width(1.0);
            ctx.stroke_rounded_rect(x, code_y, w, code_h, SETTINGS_BUTTON_RADIUS);
            ctx.set_font("700 22px monospace");
            ctx.set_fill_color(GOLD);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(code, cx, code_y + code_h / 2.0);

            let link_y = code_y + code_h + BTN_GAP;
            draw_button(
                ctx,
                coord,
                layer_id,
                "cloud_login:open_page",
                WidgetRect::new(x, link_y, w, BTN_H),
                "Open the sign-in page",
                BtnStyle::Primary,
                tt,
            );

            ctx.set_font("11px sans-serif");
            ctx.set_fill_color(&tt.item_text_muted);
            ctx.fill_text("Waiting for confirmation…", cx, link_y + BTN_H + 18.0);
        }

        CloudLoginPhase::Success { display_name } => {
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.set_font("20px sans-serif");
            ctx.set_fill_color(&tt.success);
            ctx.fill_text("✓", cx, y + 20.0);
            ctx.set_font("13px sans-serif");
            ctx.set_fill_color(&tt.item_text);
            let msg = format!("Signed in as {}", display_name);
            ctx.fill_text(&msg, cx, y + 52.0);
        }

        CloudLoginPhase::SignedIn { display_name, provider } => {
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Top);
            ctx.set_font("11px monospace");
            ctx.set_fill_color(&tt.item_text_muted);
            ctx.fill_text("SIGNED IN AS", x, y);

            ctx.set_font("600 16px sans-serif");
            ctx.set_fill_color(&tt.item_text);
            ctx.fill_text(display_name, x, y + 22.0);

            ctx.set_font("12px sans-serif");
            ctx.set_fill_color(&tt.item_text_muted);
            let via = format!("via {}", provider);
            ctx.fill_text(&via, x, y + 48.0);

            draw_button(
                ctx,
                coord,
                layer_id,
                "cloud_login:sign_out",
                WidgetRect::new(x, y + 78.0, 120.0, 32.0),
                "Sign out",
                BtnStyle::Danger,
                tt,
            );

            draw_plan_and_redeem(
                ctx, coord, layer_id, x, y + 126.0, w, state, ft, tt, current_time_ms,
            );
        }

        CloudLoginPhase::Error { message } => {
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.set_font("13px sans-serif");
            ctx.set_fill_color(&tt.danger);
            ctx.fill_text("Sign-in failed", cx, y + 10.0);
            ctx.set_font("12px sans-serif");
            ctx.set_fill_color(&tt.item_text_muted);
            // Server errors can be a whole HTML page — clip to the body width.
            let msg = crate::ui::widgets::fit_text_to_width(ctx, message, w);
            ctx.fill_text(&msg, cx, y + 34.0);

            draw_button(
                ctx,
                coord,
                layer_id,
                "cloud_login:retry",
                WidgetRect::new(x, y + 64.0, w, BTN_H),
                "Try again",
                BtnStyle::Primary,
                tt,
            );
        }
    }
}

/// The account view's lower half: which tier this account is on, and the one
/// way a person can change that today — spend a gift or trial code.
///
/// The tier is stated outright rather than implied by which surfaces happen to
/// be unlocked, so "did my code work" has an answer on the screen and not only
/// in a line that has already scrolled away.
#[allow(clippy::too_many_arguments)]
fn draw_plan_and_redeem(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    state: &CloudLoginState,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
) {
    ctx.set_fill_color(&tt.separator);
    ctx.fill_rect(x, y, w, 1.0);

    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.set_font("11px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.fill_text("PLAN", x, y + 16.0);

    // A redemption that just succeeded is newer than the entitlements answer
    // the shell is still holding, so it wins for this frame. Otherwise the
    // modal would say "Free" for the seconds the re-ask takes, immediately
    // after telling the user their code worked.
    let just_paid_until = match &state.redeem_status {
        RedeemStatus::Redeemed { paid_until } => Some(paid_until.as_str()),
        _ => None,
    };
    let until = just_paid_until.or(state.paid_until.as_deref());
    let paid = state.is_paid || just_paid_until.is_some();
    ctx.set_font("600 15px sans-serif");
    if paid {
        ctx.set_fill_color(GOLD);
        // RFC 3339 in, a date out — nobody needs the seconds.
        let day = until.map(|u| &u[..u.len().min(10)]).unwrap_or("");
        let label = if day.is_empty() {
            "Paid".to_string()
        } else {
            format!("Paid until {}", day)
        };
        ctx.fill_text(&label, x, y + 34.0);
    } else {
        ctx.set_fill_color(&tt.item_text);
        ctx.fill_text("Free", x, y + 34.0);
    }

    ctx.set_font("11px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.fill_text("GIFT OR TRIAL CODE", x, y + 64.0);

    let row_y = y + 84.0;
    let field_w = w - REDEEM_BTN_W - BTN_GAP;
    let field_rect = WidgetRect::new(x, row_y, field_w, FIELD_H);
    let wid = WidgetId::from(REDEEM_FIELD);
    let text = coord.text_fields().text(&wid).to_string();
    let cursor = coord.text_fields().cursor(&wid);
    let (sel_start, sel_end) = match coord.text_fields().selection_range(&wid) {
        Some((lo, hi)) => (Some(lo), Some(hi)),
        None => (None, None),
    };
    let engaged = coord.text_fields().is_engaged(&wid);
    let widget_theme = toolbar_to_widget_theme(tt, ft);
    let cfg = InputConfig::new(&text)
        .with_focused(engaged)
        .with_cursor(cursor)
        .with_selection(sel_start, sel_end)
        .with_placeholder("XXXXXXXXXX");
    let input_result = draw_input(ctx, &cfg, WidgetState::Normal, field_rect, &widget_theme);
    if engaged && coord.text_fields().cursor_visible(current_time_ms) {
        draw_input_cursor(
            ctx,
            input_result.cursor_x,
            input_result.cursor_y,
            input_result.cursor_height,
            &tt.item_text,
        );
    }
    coord.register_on_layer("cloud_login:redeem_input", field_rect, Sense::CLICK, layer_id);

    // An empty box has nothing to send, and a submission in flight must not go
    // twice. In both cases the button is drawn dead rather than removed, so the
    // row never reflows under the cursor that is about to click it.
    let working = state.redeem_status == RedeemStatus::Working;
    let armed = !working && !text.trim().is_empty();
    let btn_rect = WidgetRect::new(x + field_w + BTN_GAP, row_y, REDEEM_BTN_W, FIELD_H);
    if armed {
        draw_button(
            ctx,
            coord,
            layer_id,
            "cloud_login:redeem",
            btn_rect,
            "Redeem",
            BtnStyle::Primary,
            tt,
        );
    } else {
        ctx.set_fill_color(&tt.button_bg);
        ctx.fill_rounded_rect(
            btn_rect.x,
            btn_rect.y,
            btn_rect.width,
            btn_rect.height,
            SETTINGS_BUTTON_RADIUS,
        );
        ctx.set_font("600 13px sans-serif");
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(
            if working { "..." } else { "Redeem" },
            btn_rect.center_x(),
            btn_rect.center_y(),
        );
    }

    let (msg, color): (String, &str) = match &state.redeem_status {
        RedeemStatus::Idle => (String::new(), ""),
        RedeemStatus::Working => (
            "Checking the code...".to_string(),
            tt.item_text_muted.as_str(),
        ),
        RedeemStatus::Redeemed { paid_until } => {
            let day = &paid_until[..paid_until.len().min(10)];
            (
                format!("Code accepted - paid until {}", day),
                tt.success.as_str(),
            )
        }
        RedeemStatus::Failed { message } => (message.clone(), tt.danger.as_str()),
    };
    if !msg.is_empty() {
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(color);
        let fitted = crate::ui::widgets::fit_text_to_width(ctx, &msg, w);
        ctx.fill_text(&fitted, x, row_y + FIELD_H + 12.0);
    }
}

/// Button fills: the gold CTA follows the brandbook, the rest follow the
/// toolbar theme so hover/active match every other modal.
enum BtnStyle {
    Primary,
    Secondary,
    Danger,
}

/// Rounded button with a hover state + click zone.
fn draw_button(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    widget_id: &str,
    rect: WidgetRect,
    label: &str,
    style: BtnStyle,
    tt: &ToolbarTheme,
) {
    let hovered = coord.is_hovered(&WidgetId::from(widget_id));
    let (bg, fg) = match style {
        BtnStyle::Primary => (
            if hovered { GOLD_HOVER } else { GOLD },
            GOLD_TEXT,
        ),
        BtnStyle::Secondary => (
            if hovered { tt.button_bg_hover.as_str() } else { tt.dropdown_bg.as_str() },
            tt.item_text.as_str(),
        ),
        BtnStyle::Danger => (
            if hovered { tt.danger_hover_bg.as_str() } else { tt.button_bg.as_str() },
            tt.danger.as_str(),
        ),
    };
    ctx.set_fill_color(bg);
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    if matches!(style, BtnStyle::Secondary) {
        ctx.set_stroke_color(&tt.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    }
    ctx.set_font("600 13px sans-serif");
    ctx.set_fill_color(fg);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, rect.center_x(), rect.center_y());
    coord.register_on_layer(widget_id, rect, Sense::CLICK | Sense::HOVER, layer_id);
}
