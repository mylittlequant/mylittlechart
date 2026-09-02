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
use crate::ui::modal_settings::{CloudLoginPhase, CloudLoginState};
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{
    register_dimmer_zones, render_modal_shell, ModalFooter, ModalPlacement, ModalShell,
    ModalShellSpec, ModalTheme, SETTINGS_BUTTON_RADIUS,
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
        CloudLoginPhase::SignedIn { .. } => 122.0,
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
            draw_body(ctx, coord, layer_id, shell, state, toolbar_theme);
        },
    );
    CloudLoginResult { header_rect: shell.header }
}

fn draw_body(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    shell: &ModalShell,
    state: &CloudLoginState,
    tt: &ToolbarTheme,
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
