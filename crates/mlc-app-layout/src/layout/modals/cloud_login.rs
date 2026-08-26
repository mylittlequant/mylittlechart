//! Cloud sign-in / account overlay (cloud builds only).
//!
//! Centered card over a full-screen dimmer, following the welcome-wizard
//! overlay pattern (high-z layer absorbs all input). Pure presentation:
//! every interactive element just registers a `cloud_login:*` widget id —
//! the login state machine itself is driven by the shell's cloud session
//! driver through [`CloudLoginState`]'s phase field and one-shot flags.
//!
//! Visual language per the mylittlechart brandbook
//! (`zeng3ld-landing/web/mylittlechart/README.md`): gold #F4CD63 is the only
//! action accent, mono micro-labels, hairline borders, no icon fonts.

use mlc_core::engine::render::RenderContext;
use uzor::render::{TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;
use uzor::input::Sense;

use crate::ui::modal_settings::{CloudLoginPhase, CloudLoginState};
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::z_order::ZLayer;

const GOLD: &str = "#f4cd63";
const GOLD_HOVER: &str = "#f7d985";
const DARK_TEXT: &str = "#0a0f1a";
const DANGER: &str = "#c85c5c";
const SUCCESS: &str = "#5cb85c";

const MODAL_W: f64 = 430.0;
const HEADER_H: f64 = 40.0;
const PAD: f64 = 24.0;

/// Render the cloud sign-in overlay.
///
/// Returns the modal rect `(x, y, w, h)` so the caller can store it for
/// header-drag hit-testing if dragging is enabled later.
pub fn render_cloud_login_modal(
    ctx: &mut dyn RenderContext,
    window_w: f64,
    window_h: f64,
    state: &CloudLoginState,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> (f64, f64, f64, f64) {
    let layer_id = ZLayer::ModalOverlay.push_named(input_coordinator, "cloud_login");

    // Dimmer — absorbs clicks and closes the overlay (dismissable, unlike
    // the welcome wizard).
    input_coordinator.register_on_layer(
        "cloud_login:dimmer",
        WidgetRect::new(0.0, 0.0, window_w, window_h),
        Sense::CLICK,
        &layer_id,
    );
    ctx.set_fill_color("rgba(2, 4, 8, 0.72)");
    ctx.fill_rect(0.0, 0.0, window_w, window_h);

    let modal_h: f64 = match &state.phase {
        CloudLoginPhase::AwaitingConfirm { .. } => 340.0,
        CloudLoginPhase::SignedIn { .. } => 260.0,
        CloudLoginPhase::Success { .. } => 200.0,
        CloudLoginPhase::Error { .. } => 260.0,
        _ => 200.0,
    };
    let (modal_x, modal_y) = state.position.unwrap_or((
        (window_w - MODAL_W) / 2.0,
        (window_h - modal_h) / 2.0,
    ));

    // Card background + border.
    ctx.set_fill_color(toolbar_theme.background.as_str());
    ctx.fill_rounded_rect(modal_x, modal_y, MODAL_W, modal_h, 12.0);
    ctx.set_stroke_color(toolbar_theme.separator.as_str());
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.rect(modal_x + 0.5, modal_y + 0.5, MODAL_W - 1.0, modal_h - 1.0);
    ctx.stroke();

    // Absorb clicks on the card itself.
    input_coordinator.register_on_layer(
        "cloud_login:modal_bg",
        WidgetRect::new(modal_x, modal_y, MODAL_W, modal_h),
        Sense::CLICK,
        &layer_id,
    );

    // ── Header ───────────────────────────────────────────────────────────
    ctx.set_font("600 13px sans-serif");
    ctx.set_fill_color(toolbar_theme.item_text.as_str());
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    let title = match &state.phase {
        CloudLoginPhase::SignedIn { .. } => "ACCOUNT",
        _ => "SIGN IN",
    };
    ctx.fill_text(title, modal_x + PAD, modal_y + HEADER_H / 2.0);

    // Close ✕
    let close_size = 26.0;
    let close_x = modal_x + MODAL_W - close_size - 10.0;
    let close_y = modal_y + (HEADER_H - close_size) / 2.0;
    input_coordinator.register_on_layer(
        "cloud_login:close",
        WidgetRect::new(close_x, close_y, close_size, close_size),
        Sense::CLICK,
        &layer_id,
    );
    ctx.set_font("14px sans-serif");
    ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
    ctx.set_text_align(TextAlign::Center);
    ctx.fill_text("×", close_x + close_size / 2.0, close_y + close_size / 2.0);

    // Hairline under header.
    ctx.set_stroke_color(toolbar_theme.separator.as_str());
    ctx.begin_path();
    ctx.move_to(modal_x, modal_y + HEADER_H + 0.5);
    ctx.line_to(modal_x + MODAL_W, modal_y + HEADER_H + 0.5);
    ctx.stroke();

    // ── Body ─────────────────────────────────────────────────────────────
    let body_y = modal_y + HEADER_H + PAD;
    let content_w = MODAL_W - PAD * 2.0;

    match &state.phase {
        CloudLoginPhase::RequestCode | CloudLoginPhase::CreatingCode => {
            ctx.set_font("13px sans-serif");
            ctx.set_fill_color(toolbar_theme.item_text.as_str());
            ctx.set_text_align(TextAlign::Center);
            ctx.fill_text(
                "Requesting sign-in code…",
                modal_x + MODAL_W / 2.0,
                body_y + 40.0,
            );
        }

        CloudLoginPhase::AwaitingConfirm { code } => {
            ctx.set_text_align(TextAlign::Left);
            ctx.set_font("13px sans-serif");
            ctx.set_fill_color(toolbar_theme.item_text.as_str());
            ctx.fill_text("No passwords. Confirm this device", modal_x + PAD, body_y);
            ctx.fill_text("from any logged-in browser:", modal_x + PAD, body_y + 20.0);

            // Code card — mono, gold.
            let code_y = body_y + 36.0;
            let code_h = 52.0;
            ctx.set_fill_color(toolbar_theme.dropdown_bg.as_str());
            ctx.fill_rounded_rect(modal_x + PAD, code_y, content_w, code_h, 8.0);
            ctx.set_font("700 26px monospace");
            ctx.set_fill_color(GOLD);
            ctx.set_text_align(TextAlign::Center);
            ctx.fill_text(code, modal_x + MODAL_W / 2.0, code_y + code_h / 2.0);

            // Open-page button (primary, gold).
            let btn_y = code_y + code_h + 16.0;
            let btn_h = 34.0;
            draw_button(
                ctx,
                input_coordinator,
                &layer_id,
                "cloud_login:open_page",
                modal_x + PAD,
                btn_y,
                content_w,
                btn_h,
                "Open the sign-in page →",
                GOLD,
                DARK_TEXT,
            );

            ctx.set_font("11px sans-serif");
            ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
            ctx.fill_text(
                "Waiting for confirmation…",
                modal_x + MODAL_W / 2.0,
                btn_y + btn_h + 22.0,
            );
        }

        CloudLoginPhase::Success { display_name } => {
            ctx.set_font("20px sans-serif");
            ctx.set_fill_color(SUCCESS);
            ctx.set_text_align(TextAlign::Center);
            ctx.fill_text("✓", modal_x + MODAL_W / 2.0, body_y + 24.0);
            ctx.set_font("13px sans-serif");
            ctx.set_fill_color(toolbar_theme.item_text.as_str());
            let msg = format!("Signed in as {}", display_name);
            ctx.fill_text(&msg, modal_x + MODAL_W / 2.0, body_y + 56.0);
        }

        CloudLoginPhase::SignedIn { display_name, provider } => {
            ctx.set_text_align(TextAlign::Left);
            ctx.set_font("11px monospace");
            ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
            ctx.fill_text("SIGNED IN AS", modal_x + PAD, body_y);

            ctx.set_font("600 16px sans-serif");
            ctx.set_fill_color(toolbar_theme.item_text.as_str());
            ctx.fill_text(display_name, modal_x + PAD, body_y + 26.0);

            ctx.set_font("12px sans-serif");
            ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
            let via = format!("via {}", provider);
            ctx.fill_text(&via, modal_x + PAD, body_y + 48.0);

            // Sign out (ghost, danger on hover).
            let btn_y = body_y + 72.0;
            let btn_h = 32.0;
            draw_button(
                ctx,
                input_coordinator,
                &layer_id,
                "cloud_login:sign_out",
                modal_x + PAD,
                btn_y,
                120.0,
                btn_h,
                "Sign out",
                toolbar_theme.button_bg.as_str(),
                DANGER,
            );
        }

        CloudLoginPhase::Error { message } => {
            ctx.set_font("13px sans-serif");
            ctx.set_fill_color(DANGER);
            ctx.set_text_align(TextAlign::Center);
            ctx.fill_text("Sign-in failed", modal_x + MODAL_W / 2.0, body_y + 8.0);
            ctx.set_font("12px sans-serif");
            ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
            ctx.fill_text(message, modal_x + MODAL_W / 2.0, body_y + 32.0);

            let btn_y = body_y + 56.0;
            let btn_h = 32.0;
            draw_button(
                ctx,
                input_coordinator,
                &layer_id,
                "cloud_login:retry",
                modal_x + PAD,
                btn_y,
                content_w,
                btn_h,
                "Try again",
                GOLD,
                DARK_TEXT,
            );
        }
    }

    (modal_x, modal_y, MODAL_W, modal_h)
}

/// Solid rounded button + click zone. Hover styling is driven by the
/// coordinator via the widget id; v1 keeps a flat fill.
#[allow(clippy::too_many_arguments)]
fn draw_button(
    ctx: &mut dyn RenderContext,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    widget_id: &str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    label: &str,
    bg: &str,
    fg: &str,
) {
    let _ = GOLD_HOVER; // reserved for hover state wiring
    ctx.set_fill_color(bg);
    ctx.fill_rounded_rect(x, y, w, h, 8.0);
    ctx.set_font("600 13px sans-serif");
    ctx.set_fill_color(fg);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, x + w / 2.0, y + h / 2.0);
    input_coordinator.register_on_layer(
        widget_id,
        WidgetRect::new(x, y, w, h),
        Sense::CLICK,
        layer_id,
    );
}
