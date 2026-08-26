//! News / Links / Download overlays (cloud builds only).
//!
//! Standard modal shell (`ui::widgets::modal`) over a dimmed backdrop, one
//! dialog per [`CloudOverlayKind`]:
//! - News — "what's new": version + release notes from /api/updates/latest
//!   (fetched by the shell driver into [`CloudOverlayData`]), plus a row out
//!   to the blog where the long-form posts live.
//! - Links — social/project links, opened in a new tab via the
//!   `open_url_requested` mailbox.
//! - Download — desktop binary download link from the same manifest.
//!
//! Pure presentation: all effects live in the shell's cloud session driver.

use mlc_core::engine::render::{draw_svg_icon, RenderContext, TextAlign, TextBaseline};
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::types::Rect as WidgetRect;
use uzor::WidgetId;

use crate::layout::render_chart::FrameTheme;
use crate::ui::modal_settings::{CloudOverlayData, CloudOverlayKind, CloudOverlayState};
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{
    register_dimmer_zones, render_modal_shell, ModalPlacement, ModalShell, ModalShellSpec,
    ModalTheme, SETTINGS_BUTTON_RADIUS,
};
use crate::ui::z_order::ZLayer;
use crate::ui::Icon;

const GOLD: &str = "#f4cd63";

const MODAL_W: f64 = 420.0;
const ROW_H: f64 = 40.0;
/// Gap between stacked rows.
const ROW_GAP: f64 = 6.0;

/// Social/project links shown in the Links overlay (id, label, URL).
/// Keep in sync with the landing footer. The click handler resolves ids
/// through this table, so a row anywhere (News → blog) can reuse them.
pub const SOCIAL_LINKS: &[(&str, &str, &str)] = &[
    ("github", "GitHub", "https://github.com/knyaz-tishini/mylittlechart"),
    ("x", "X / Twitter", "https://x.com/mylittlechart"),
    ("telegram", "Telegram", "https://t.me/mylittlechart"),
    ("discord", "Discord", "https://discord.gg/mylittlechart"),
    ("blog", "Blog", "https://chart.example/blog"),
    ("donate", "Support the project", "https://chart.example/donate"),
];

/// Geometry the input side needs (header drag).
pub struct CloudOverlayResult {
    pub header_rect: WidgetRect,
}

fn title_for(kind: CloudOverlayKind) -> &'static str {
    match kind {
        CloudOverlayKind::News => "What's new",
        CloudOverlayKind::Links => "Links",
        CloudOverlayKind::Download => "Download",
    }
}

/// Estimated wrapped-line count for the release notes at the body's width.
/// ~58 chars per line at 12px over ~390px of content.
fn news_lines(notes: &str) -> f64 {
    (notes.len() as f64 / 58.0).ceil().max(1.0)
}

/// Body height per kind — the shell sizes the frame from this.
fn content_height(state: &CloudOverlayState) -> f64 {
    match state.kind {
        CloudOverlayKind::Links => SOCIAL_LINKS.len() as f64 * ROW_H + 4.0,
        CloudOverlayKind::Download => 22.0 + ROW_H + 24.0,
        CloudOverlayKind::News => match &state.data {
            CloudOverlayData::Ready { notes, .. } => {
                26.0 + news_lines(notes) * 18.0 + 12.0 + ROW_H
            }
            _ => 84.0,
        },
    }
}

/// Render the cloud overlay (News / Links / Download).
pub fn render_cloud_overlay(
    ctx: &mut dyn RenderContext,
    window_w: f64,
    window_h: f64,
    state: &CloudOverlayState,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut InputCoordinator,
) -> CloudOverlayResult {
    let theme = ModalTheme::from_toolbar(
        &frame_theme.toolbar_bg,
        &toolbar_theme.separator,
        toolbar_theme,
    );
    let spec = ModalShellSpec {
        dim_backdrop: true,
        placement: ModalPlacement::At(state.position),
        layer: ZLayer::ModalOverlay,
        ..ModalShellSpec::new(
            "cloud_overlay",
            title_for(state.kind),
            MODAL_W,
            content_height(state),
        )
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
                "cloud_overlay",
                shell.frame,
                (window_w, window_h),
            );
            draw_body(ctx, coord, layer_id, shell, state, toolbar_theme);
        },
    );
    CloudOverlayResult { header_rect: shell.header }
}

fn draw_body(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    shell: &ModalShell,
    state: &CloudOverlayState,
    tt: &ToolbarTheme,
) {
    let x = shell.content.x;
    let y = shell.content.y;
    let w = shell.content.width;
    let row_w = w;

    match state.kind {
        CloudOverlayKind::Links => {
            for (i, (id, label, _url)) in SOCIAL_LINKS.iter().enumerate() {
                draw_link_row(
                    ctx,
                    coord,
                    layer_id,
                    &format!("cloud_overlay:link:{}", id),
                    WidgetRect::new(x, y + i as f64 * ROW_H, row_w, ROW_H - ROW_GAP),
                    label,
                    Icon::Globe,
                    tt,
                );
            }
        }

        CloudOverlayKind::Download => match &state.data {
            CloudOverlayData::Ready { version, .. } => {
                ctx.set_font("12px sans-serif");
                ctx.set_fill_color(&tt.item_text_muted);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Top);
                let cap = format!("Desktop app — latest release v{}", version);
                ctx.fill_text(&cap, x, y);
                draw_link_row(
                    ctx,
                    coord,
                    layer_id,
                    "cloud_overlay:download:win64",
                    WidgetRect::new(x, y + 22.0, row_w, ROW_H - ROW_GAP),
                    "Windows (x64)",
                    Icon::CloudDownload,
                    tt,
                );
                ctx.set_font("11px sans-serif");
                ctx.set_fill_color(&tt.item_text_muted);
                ctx.fill_text(
                    "The desktop app updates itself (OTA) once installed.",
                    x,
                    y + 22.0 + ROW_H + 6.0,
                );
            }
            CloudOverlayData::Error { message } => {
                draw_status(ctx, tt, x, y, w, "Failed to load release info", Some(message));
            }
            _ => {
                draw_status(ctx, tt, x, y, w, "Loading release info…", None);
            }
        },

        CloudOverlayKind::News => match &state.data {
            CloudOverlayData::Ready { version, notes, .. } => {
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Top);
                ctx.set_font("700 14px sans-serif");
                ctx.set_fill_color(GOLD);
                let v = format!("v{}", version);
                ctx.fill_text(&v, x, y);
                ctx.set_font("12px sans-serif");
                ctx.set_fill_color(&tt.item_text);
                let mut ly = y + 26.0;
                for line in wrap_text(notes, 58) {
                    ctx.fill_text(&line, x, ly);
                    ly += 18.0;
                }
                draw_link_row(
                    ctx,
                    coord,
                    layer_id,
                    "cloud_overlay:link:blog",
                    WidgetRect::new(x, ly + 12.0, row_w, ROW_H - ROW_GAP),
                    "All news on the blog",
                    Icon::Globe,
                    tt,
                );
            }
            CloudOverlayData::Error { message } => {
                draw_status(ctx, tt, x, y, w, "Failed to load news", Some(message));
            }
            _ => {
                draw_status(ctx, tt, x, y, w, "Loading…", None);
            }
        },
    }
}

/// One clickable row: SVG icon + label + trailing "opens externally" glyph,
/// with the standard hover surface.
#[allow(clippy::too_many_arguments)]
fn draw_link_row(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    widget_id: &str,
    rect: WidgetRect,
    label: &str,
    icon: Icon,
    tt: &ToolbarTheme,
) {
    let hovered = coord.is_hovered(&WidgetId::from(widget_id));
    ctx.set_fill_color(if hovered { &tt.item_bg_hover } else { &tt.dropdown_bg });
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);

    let icon_tint = if hovered { &tt.item_text } else { &tt.item_text_muted };
    draw_svg_icon(
        ctx,
        icon.svg(),
        rect.x + 10.0,
        rect.y + (rect.height - 18.0) / 2.0,
        18.0,
        18.0,
        icon_tint,
    );

    ctx.set_font("13px sans-serif");
    ctx.set_fill_color(&tt.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, rect.x + 38.0, rect.center_y());

    draw_svg_icon(
        ctx,
        Icon::NewWindow.svg(),
        rect.right() - 26.0,
        rect.y + (rect.height - 14.0) / 2.0,
        14.0,
        14.0,
        &tt.item_text_muted,
    );

    coord.register_on_layer(widget_id, rect, Sense::CLICK | Sense::HOVER, layer_id);
}

fn draw_status(
    ctx: &mut dyn RenderContext,
    tt: &ToolbarTheme,
    x: f64,
    y: f64,
    w: f64,
    headline: &str,
    detail: Option<&str>,
) {
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_font("13px sans-serif");
    ctx.set_fill_color(&tt.item_text);
    ctx.fill_text(headline, x + w / 2.0, y + 20.0);
    if let Some(d) = detail {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&tt.item_text_muted);
        // Server errors can be a whole HTML page — clip to the body width.
        let d = crate::ui::widgets::fit_text_to_width(ctx, d, w);
        ctx.fill_text(&d, x + w / 2.0, y + 44.0);
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
