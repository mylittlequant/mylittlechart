//! News / Links / Download overlays (cloud builds only).
//!
//! One draggable card over a dimmer, three kinds ([`CloudOverlayKind`]):
//! - News — "what's new": version + release notes from /api/updates/latest
//!   (fetched by the shell driver into [`CloudOverlayData`]).
//! - Links — social/project links, opened in a new tab via the
//!   `open_url_requested` mailbox.
//! - Download — desktop binary download link from the same manifest.
//!
//! Pure presentation: all effects live in the shell's cloud session driver.

use mlc_core::engine::render::{draw_svg_icon, RenderContext};
use mlc_core::state::icons::icon_svg;
use uzor::render::{TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;
use uzor::input::Sense;

use crate::ui::modal_settings::{CloudOverlayData, CloudOverlayKind, CloudOverlayState};
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::z_order::ZLayer;

const GOLD: &str = "#f4cd63";

const MODAL_W: f64 = 420.0;
const HEADER_H: f64 = 40.0;
const PAD: f64 = 20.0;
const ROW_H: f64 = 40.0;

/// Social/project links shown in the Links overlay (id, label, URL).
/// Keep in sync with the landing footer.
pub const SOCIAL_LINKS: &[(&str, &str, &str)] = &[
    ("github", "GitHub", "https://github.com/knyaz-tishini/mylittlechart"),
    ("x", "X / Twitter", "https://x.com/mylittlechart"),
    ("telegram", "Telegram", "https://t.me/mylittlechart"),
    ("discord", "Discord", "https://discord.gg/mylittlechart"),
];

/// Geometry the input side needs (header drag).
pub struct CloudOverlayResult {
    pub header_rect: WidgetRect,
}

fn title_for(kind: CloudOverlayKind) -> &'static str {
    match kind {
        CloudOverlayKind::News => "WHAT'S NEW",
        CloudOverlayKind::Links => "LINKS",
        CloudOverlayKind::Download => "DOWNLOAD",
    }
}

fn body_height(state: &CloudOverlayState) -> f64 {
    match state.kind {
        CloudOverlayKind::Links => SOCIAL_LINKS.len() as f64 * ROW_H + PAD,
        CloudOverlayKind::Download => ROW_H + 60.0,
        CloudOverlayKind::News => match &state.data {
            CloudOverlayData::Ready { notes, .. } => {
                // ~4 chars per px at 12px font over 380px content width.
                let lines = (notes.len() as f64 / 58.0).ceil().max(1.0) + 2.0;
                (lines * 18.0 + 40.0).min(360.0)
            }
            _ => 120.0,
        },
    }
}

/// Render the cloud overlay (News / Links / Download).
pub fn render_cloud_overlay(
    ctx: &mut dyn RenderContext,
    window_w: f64,
    window_h: f64,
    state: &CloudOverlayState,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> CloudOverlayResult {
    let layer_id = ZLayer::ModalOverlay.push_named(input_coordinator, "cloud_overlay");

    input_coordinator.register_on_layer(
        "cloud_overlay:dimmer",
        WidgetRect::new(0.0, 0.0, window_w, window_h),
        Sense::CLICK,
        &layer_id,
    );
    ctx.set_fill_color("rgba(2, 4, 8, 0.72)");
    ctx.fill_rect(0.0, 0.0, window_w, window_h);

    let modal_h = HEADER_H + body_height(state) + PAD;
    let (modal_x, modal_y) = state.position.unwrap_or((
        (window_w - MODAL_W) / 2.0,
        (window_h - modal_h) / 2.0,
    ));

    ctx.set_fill_color(toolbar_theme.background.as_str());
    ctx.fill_rounded_rect(modal_x, modal_y, MODAL_W, modal_h, 12.0);
    ctx.set_stroke_color(toolbar_theme.separator.as_str());
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.rect(modal_x + 0.5, modal_y + 0.5, MODAL_W - 1.0, modal_h - 1.0);
    ctx.stroke();

    input_coordinator.register_on_layer(
        "cloud_overlay:modal_bg",
        WidgetRect::new(modal_x, modal_y, MODAL_W, modal_h),
        Sense::CLICK,
        &layer_id,
    );

    // Header (draggable) + close.
    let header_rect = WidgetRect::new(modal_x, modal_y, MODAL_W, HEADER_H);
    input_coordinator.register_on_layer(
        "cloud_overlay:header",
        header_rect,
        Sense::DRAG,
        &layer_id,
    );
    ctx.set_font("600 13px sans-serif");
    ctx.set_fill_color(toolbar_theme.item_text.as_str());
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(title_for(state.kind), modal_x + PAD, modal_y + HEADER_H / 2.0);

    let close_size = 26.0;
    let close_x = modal_x + MODAL_W - close_size - 10.0;
    let close_y = modal_y + (HEADER_H - close_size) / 2.0;
    input_coordinator.register_on_layer(
        "cloud_overlay:close",
        WidgetRect::new(close_x, close_y, close_size, close_size),
        Sense::CLICK,
        &layer_id,
    );
    ctx.set_font("14px sans-serif");
    ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
    ctx.set_text_align(TextAlign::Center);
    ctx.fill_text("×", close_x + close_size / 2.0, close_y + close_size / 2.0);

    ctx.set_stroke_color(toolbar_theme.separator.as_str());
    ctx.begin_path();
    ctx.move_to(modal_x, modal_y + HEADER_H + 0.5);
    ctx.line_to(modal_x + MODAL_W, modal_y + HEADER_H + 0.5);
    ctx.stroke();

    let body_y = modal_y + HEADER_H + PAD * 0.5;
    let content_w = MODAL_W - PAD * 2.0;

    match state.kind {
        CloudOverlayKind::Links => {
            for (i, (id, label, _url)) in SOCIAL_LINKS.iter().enumerate() {
                let row_y = body_y + i as f64 * ROW_H;
                draw_row(
                    ctx,
                    input_coordinator,
                    &layer_id,
                    &format!("cloud_overlay:link:{}", id),
                    modal_x + PAD,
                    row_y,
                    content_w,
                    label,
                    "globe",
                    toolbar_theme,
                );
            }
        }

        CloudOverlayKind::Download => match &state.data {
            CloudOverlayData::Ready { version, download_url, .. } => {
                ctx.set_font("12px sans-serif");
                ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
                ctx.set_text_align(TextAlign::Left);
                let cap = format!("Desktop app — latest release v{}", version);
                ctx.fill_text(&cap, modal_x + PAD, body_y + 6.0);
                draw_row(
                    ctx,
                    input_coordinator,
                    &layer_id,
                    "cloud_overlay:download:win64",
                    modal_x + PAD,
                    body_y + 20.0,
                    content_w,
                    "Windows (x64)",
                    "cloud_download",
                    toolbar_theme,
                );
                let _ = download_url;
            }
            CloudOverlayData::Error { message } => {
                draw_status(ctx, toolbar_theme, modal_x, body_y, "Failed to load release info", Some(message));
            }
            _ => {
                draw_status(ctx, toolbar_theme, modal_x, body_y, "Loading release info…", None);
            }
        },

        CloudOverlayKind::News => match &state.data {
            CloudOverlayData::Ready { version, notes, .. } => {
                ctx.set_font("700 14px sans-serif");
                ctx.set_fill_color(GOLD);
                ctx.set_text_align(TextAlign::Left);
                let v = format!("v{}", version);
                ctx.fill_text(&v, modal_x + PAD, body_y + 8.0);
                // Word-wrapped release notes, 12px, ~58 chars/line.
                ctx.set_font("12px sans-serif");
                ctx.set_fill_color(toolbar_theme.item_text.as_str());
                let mut y = body_y + 32.0;
                for line in wrap_text(notes, 58) {
                    ctx.fill_text(&line, modal_x + PAD, y);
                    y += 18.0;
                }
            }
            CloudOverlayData::Error { message } => {
                draw_status(ctx, toolbar_theme, modal_x, body_y, "Failed to load news", Some(message));
            }
            _ => {
                draw_status(ctx, toolbar_theme, modal_x, body_y, "Loading…", None);
            }
        },
    }

    CloudOverlayResult { header_rect }
}

/// One clickable row: icon + label + hover surface.
#[allow(clippy::too_many_arguments)]
fn draw_row(
    ctx: &mut dyn RenderContext,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    widget_id: &str,
    x: f64,
    y: f64,
    w: f64,
    label: &str,
    icon: &str,
    toolbar_theme: &ToolbarTheme,
) {
    ctx.set_fill_color(toolbar_theme.dropdown_bg.as_str());
    ctx.fill_rounded_rect(x, y, w, ROW_H - 6.0, 8.0);
    if let Some(svg) = icon_svg(icon) {
        draw_svg_icon(ctx, svg, x + 10.0, y + 7.0, 20.0, 20.0, &toolbar_theme.item_text);
    }
    ctx.set_font("13px sans-serif");
    ctx.set_fill_color(toolbar_theme.item_text.as_str());
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, x + 40.0, y + (ROW_H - 6.0) / 2.0);
    // Trailing arrow.
    ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
    ctx.set_text_align(TextAlign::Right);
    ctx.fill_text("→", x + w - 12.0, y + (ROW_H - 6.0) / 2.0);
    input_coordinator.register_on_layer(
        widget_id,
        WidgetRect::new(x, y, w, ROW_H - 6.0),
        Sense::CLICK,
        layer_id,
    );
}

fn draw_status(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    modal_x: f64,
    body_y: f64,
    headline: &str,
    detail: Option<&str>,
) {
    ctx.set_font("13px sans-serif");
    ctx.set_fill_color(toolbar_theme.item_text.as_str());
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(headline, modal_x + PAD, body_y + 16.0);
    if let Some(d) = detail {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
        ctx.fill_text(d, modal_x + PAD, body_y + 38.0);
    }
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for raw in text.lines() {
        let mut rest = raw.trim();
        while rest.len() > width {
            let cut = rest[..width].rfind(' ').unwrap_or(width);
            lines.push(rest[..cut].trim().to_string());
            rest = rest[cut..].trim();
        }
        lines.push(rest.to_string());
    }
    lines
}
