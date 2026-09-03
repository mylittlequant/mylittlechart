//! News / Links / Download overlays (cloud builds only).
//!
//! Standard modal shell (`ui::widgets::modal`) over a dimmed backdrop, one
//! dialog per [`CloudOverlayKind`]:
//! - News — "what's new": version + release notes from /api/updates/latest
//!   (fetched by the shell driver into [`CloudOverlayData`]), plus a row out
//!   to the blog where the long-form posts live.
//! - Links — social/project links, opened in a new tab via the
//!   `open_url_requested` mailbox.
//! - Download — the release manifest, read by whichever surface asked.
//!   The web page gets binary download links; the desktop app gets its own
//!   updater's state and an Install action, because there the app replaces
//!   its own binary and a link would be busywork.
//!
//! Pure presentation: all effects live in the shell's cloud session driver.

use mlc_core::engine::render::{draw_svg_icon, RenderContext, TextAlign, TextBaseline};
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::types::Rect as WidgetRect;
use uzor::WidgetId;

use crate::layout::render_chart::FrameTheme;
use crate::ui::modal_settings::{
    CloudNewsPost, CloudOverlayData, CloudOverlayKind, CloudOverlayState, CloudReleaseEntry,
    UpdateOverlayPhase,
};
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

fn title_for(state: &CloudOverlayState) -> &'static str {
    match state.kind {
        CloudOverlayKind::News => "What's new",
        CloudOverlayKind::Links => "Links",
        // Same feed, but the desktop build acts on it instead of linking to
        // it, and the title has to say which of the two the user is looking
        // at — "Download" over an Install button reads like a mistake.
        CloudOverlayKind::Download => match &state.data {
            CloudOverlayData::Update { .. } => "Update",
            _ => "Download",
        },
    }
}

/// Estimated wrapped-line count for a post summary at the body's width.
/// ~62 chars per line at 12px over ~390px of content, capped at two lines.
fn summary_lines(summary: &str) -> f64 {
    (summary.len() as f64 / 62.0).ceil().clamp(1.0, 2.0)
}

/// Height of one News post block: date + title + summary + breathing room.
fn post_block_h(post: &CloudNewsPost) -> f64 {
    16.0 + 20.0 + summary_lines(&post.summary) * 16.0 + 14.0
}

/// Height of the News reading view: back row + date + title + wrapped body.
fn post_detail_h(post: &CloudNewsPost) -> f64 {
    let text = if post.body.is_empty() { &post.summary } else { &post.body };
    let mut h = 0.0;
    for par in text.split("\n\n") {
        h += wrap_text(par.trim(), 62).len() as f64 * 16.0 + 10.0;
    }
    28.0 + 16.0 + 24.0 + h
}

/// The Download overlay's expanded list shows at most this many builds.
/// The modal grows with its content and cannot scroll, so this is what
/// keeps it inside the window as release history accumulates.
const MAX_RELEASE_ROWS: usize = 12;

/// The News overlay shows at most this many posts.
const MAX_NEWS_POSTS: usize = 3;

/// Body height per kind — the shell sizes the frame from this.
fn content_height(state: &CloudOverlayState) -> f64 {
    match state.kind {
        CloudOverlayKind::Links => SOCIAL_LINKS.len() as f64 * ROW_H + 4.0,
        CloudOverlayKind::Download => match &state.data {
            // Update view: caption + notes + the action row (or, in the
            // phases that have no action, caption + a status line).
            CloudOverlayData::Update { phase } => match phase {
                UpdateOverlayPhase::Available { notes, .. } => {
                    22.0 + wrap_text(notes, 62).len().min(4) as f64 * 16.0 + 10.0 + ROW_H
                }
                UpdateOverlayPhase::Checking | UpdateOverlayPhase::UpToDate { .. } => 22.0 + 24.0,
                UpdateOverlayPhase::RestartPending => 22.0 + ROW_H + 8.0,
                UpdateOverlayPhase::Failed { .. } => 22.0 + 24.0 + ROW_H,
                _ => 22.0 + 24.0,
            },
            // Web view: caption + primary action + OTA note + the row that
            // expands the modal, plus — only while expanded — one row per
            // `release_rows` entry. Shared arithmetic with `draw_body` via
            // `download_ready_height`/`release_rows` so the frame can never
            // size differently from what actually gets drawn into it.
            CloudOverlayData::Ready { platform, releases, .. } => {
                download_ready_height(state.expanded, release_rows(platform, releases).len())
            }
            _ => 22.0 + ROW_H + 24.0,
        },
        CloudOverlayKind::News => match &state.data {
            CloudOverlayData::News { posts } => {
                // Reading view: one post, expanded. List view: up to
                // MAX_NEWS_POSTS blocks + the blog row.
                if let Some(post) = state.selected_post.and_then(|i| posts.get(i)) {
                    post_detail_h(post)
                } else {
                    let blocks: f64 = posts
                        .iter()
                        .take(MAX_NEWS_POSTS)
                        .map(post_block_h)
                        .sum();
                    blocks + 8.0 + ROW_H
                }
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
        &frame_theme.modal_bg,
        &toolbar_theme.separator,
        toolbar_theme,
    );
    let spec = ModalShellSpec {
        dim_backdrop: true,
        placement: ModalPlacement::At(state.position),
        layer: ZLayer::ModalOverlay,
        ..ModalShellSpec::new(
            "cloud_overlay",
            title_for(state),
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
            // Desktop build: the updater is already running against this
            // manifest in the same process, so the overlay is a view of it.
            CloudOverlayData::Update { phase } => {
                draw_update_phase(ctx, coord, layer_id, x, y, row_w, phase, tt);
            }
            CloudOverlayData::Ready { platform, version, releases, .. } => {
                ctx.set_font("12px sans-serif");
                ctx.set_fill_color(&tt.item_text_muted);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Top);
                let cap = format!("{} — latest release v{}", platform_label(platform), version);
                ctx.fill_text(&cap, x, y);

                // Primary action: THIS visitor's own build, straight away —
                // no dropdown, no picking a platform first.
                draw_action_row(
                    ctx,
                    coord,
                    layer_id,
                    "cloud_overlay:download:primary",
                    WidgetRect::new(x, y + 22.0, row_w, ROW_H - ROW_GAP),
                    &format!("Download for {}", platform_label(platform)),
                    tt,
                );

                ctx.set_font("11px sans-serif");
                ctx.set_fill_color(&tt.item_text_muted);
                let note_y = y + 22.0 + ROW_H + 6.0;
                ctx.fill_text(
                    "The desktop app updates itself (OTA) once installed.",
                    x,
                    note_y,
                );

                // The row that expands the modal into the full catalogue —
                // every other platform's current build, then older versions.
                let rows = release_rows(platform, releases);
                let toggle_y = note_y + 14.0;
                let toggle_label = if state.expanded {
                    "Hide other platforms & versions"
                } else {
                    "Other platforms & versions"
                };
                draw_expand_row(
                    ctx,
                    coord,
                    layer_id,
                    "cloud_overlay:toggle_expand",
                    WidgetRect::new(x, toggle_y, row_w, ROW_H - ROW_GAP),
                    toggle_label,
                    state.expanded,
                    tt,
                );

                if state.expanded {
                    let mut ry = toggle_y + ROW_H + 8.0;
                    for r in rows {
                        draw_link_row(
                            ctx,
                            coord,
                            layer_id,
                            &format!("cloud_overlay:download:release:{}:{}", r.platform, r.version),
                            WidgetRect::new(x, ry, row_w, ROW_H - ROW_GAP),
                            &format!(
                                "{} — v{} — {} — {}",
                                platform_label(&r.platform),
                                r.version,
                                format_file_size_mb(r.file_size),
                                format_release_date(&r.created_at),
                            ),
                            Icon::CloudDownload,
                            tt,
                        );
                        ry += ROW_H;
                    }
                }
            }
            CloudOverlayData::Error { message } => {
                draw_status(ctx, tt, x, y, w, "Failed to load release info", Some(message));
            }
            _ => {
                draw_status(ctx, tt, x, y, w, "Loading release info…", None);
            }
        },

        CloudOverlayKind::News => match &state.data {
            CloudOverlayData::News { posts } => {
                if let Some(post) = state.selected_post.and_then(|i| posts.get(i)) {
                    draw_news_detail(ctx, coord, layer_id, x, y, w, post, tt);
                } else {
                    let mut py = y;
                    for (i, post) in posts.iter().take(MAX_NEWS_POSTS).enumerate() {
                        py += draw_news_post(
                            ctx,
                            coord,
                            layer_id,
                            &format!("cloud_overlay:post:{}", i),
                            x,
                            py,
                            w,
                            post,
                            tt,
                        );
                    }
                    draw_link_row(
                        ctx,
                        coord,
                        layer_id,
                        "cloud_overlay:link:blog",
                        WidgetRect::new(x, py + 8.0, row_w, ROW_H - ROW_GAP),
                        "All news on the blog",
                        Icon::Globe,
                        tt,
                    );
                }
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

/// One blog post block in the News overlay: date, title, summary — the whole
/// block is a click zone out to the post. Returns the height consumed.
#[allow(clippy::too_many_arguments)]
fn draw_news_post(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    widget_id: &str,
    x: f64,
    y: f64,
    w: f64,
    post: &CloudNewsPost,
    tt: &ToolbarTheme,
) -> f64 {
    let h = post_block_h(post);
    let rect = WidgetRect::new(x, y, w, h - 6.0);
    let hovered = coord.is_hovered(&WidgetId::from(widget_id));
    if hovered {
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    }

    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.set_font("10px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.fill_text(&post.date, x + 8.0, y + 6.0);

    ctx.set_font("600 13px sans-serif");
    ctx.set_fill_color(if hovered { GOLD } else { &tt.item_text });
    ctx.fill_text(&post.title, x + 8.0, y + 22.0);

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&tt.item_text_muted);
    let mut ly = y + 44.0;
    for line in wrap_text(&post.summary, 62).into_iter().take(2) {
        ctx.fill_text(&line, x + 8.0, ly);
        ly += 16.0;
    }

    coord.register_on_layer(widget_id, rect, Sense::CLICK | Sense::HOVER, layer_id);
    h
}

/// The News reading view: one post expanded in place — back row on top,
/// then date, title and the full body (falls back to the summary when the
/// feed entry has no body). The modal grows to fit via `post_detail_h`.
#[allow(clippy::too_many_arguments)]
fn draw_news_detail(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    post: &CloudNewsPost,
    tt: &ToolbarTheme,
) {
    // Back row — returns to the post list.
    let back_id = "cloud_overlay:news_back";
    let back_rect = WidgetRect::new(x, y, w, 28.0);
    let hovered = coord.is_hovered(&WidgetId::from(back_id));
    if hovered {
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.fill_rounded_rect(back_rect.x, back_rect.y, back_rect.width, back_rect.height, SETTINGS_BUTTON_RADIUS);
    }
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(if hovered { &tt.item_text } else { &tt.item_text_muted });
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("← All news", x + 8.0, y + 14.0);
    coord.register_on_layer(back_id, back_rect, Sense::CLICK | Sense::HOVER, layer_id);

    let mut ly = y + 36.0;
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.set_font("10px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.fill_text(&post.date, x, ly);
    ly += 16.0;

    ctx.set_font("600 14px sans-serif");
    ctx.set_fill_color(GOLD);
    ctx.fill_text(&post.title, x, ly);
    ly += 26.0;

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&tt.item_text);
    let text = if post.body.is_empty() { &post.summary } else { &post.body };
    for par in text.split("\n\n") {
        for line in wrap_text(par.trim(), 62) {
            ctx.fill_text(&line, x, ly);
            ly += 16.0;
        }
        ly += 10.0;
    }
}

/// The Download overlay on a desktop build: the in-process updater's state.
///
/// Only two phases carry an action. `Available` offers Install, which posts
/// the `update_install_requested` mailbox for the shell to turn into an
/// updater command; `Failed` offers a retry through the same mailbox, since
/// re-running the install IS the retry. Everything between them is the
/// updater working on its own — a progress line, nothing to press.
#[allow(clippy::too_many_arguments)]
fn draw_update_phase(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    row_w: f64,
    phase: &UpdateOverlayPhase,
    tt: &ToolbarTheme,
) {
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);

    match phase {
        UpdateOverlayPhase::Checking => {
            ctx.fill_text("Checking for updates…", x, y);
        }

        UpdateOverlayPhase::UpToDate { version } => {
            ctx.fill_text(&format!("You are on the latest release, v{version}."), x, y);
        }

        UpdateOverlayPhase::Available { version, notes } => {
            ctx.fill_text(&format!("Version {version} is available."), x, y);
            let mut ny = y + 22.0;
            ctx.set_font("12px sans-serif");
            ctx.set_fill_color(&tt.item_text);
            for line in wrap_text(notes, 62).into_iter().take(4) {
                ctx.fill_text(&line, x, ny);
                ny += 16.0;
            }
            draw_action_row(
                ctx,
                coord,
                layer_id,
                "cloud_overlay:update:install",
                WidgetRect::new(x, ny + 10.0, row_w, ROW_H - ROW_GAP),
                &format!("Install v{version}"),
                tt,
            );
        }

        UpdateOverlayPhase::Downloading { percent } => {
            ctx.fill_text(&format!("Downloading… {percent}%"), x, y);
            // A bar rather than a spinner: this is the one phase whose length
            // the updater actually knows.
            let track_y = y + 26.0;
            ctx.set_fill_color(&tt.dropdown_bg);
            ctx.fill_rounded_rect(x, track_y, row_w, 6.0, 3.0);
            ctx.set_fill_color(GOLD);
            ctx.fill_rounded_rect(x, track_y, row_w * (*percent as f64 / 100.0), 6.0, 3.0);
        }

        UpdateOverlayPhase::Verifying => {
            ctx.fill_text("Verifying the download…", x, y);
        }

        UpdateOverlayPhase::Installing => {
            ctx.fill_text("Installing…", x, y);
        }

        UpdateOverlayPhase::RestartPending => {
            ctx.fill_text("Installed. It takes effect the next time you start the app.", x, y);
        }

        UpdateOverlayPhase::Failed { message } => {
            ctx.set_fill_color(&tt.item_text);
            ctx.fill_text("The update did not complete.", x, y);
            ctx.set_font("11px sans-serif");
            ctx.set_fill_color(&tt.item_text_muted);
            let m = crate::ui::widgets::fit_text_to_width(ctx, message, row_w);
            ctx.fill_text(&m, x, y + 20.0);
            draw_action_row(
                ctx,
                coord,
                layer_id,
                "cloud_overlay:update:install",
                WidgetRect::new(x, y + 44.0, row_w, ROW_H - ROW_GAP),
                "Try again",
                tt,
            );
        }
    }
}

/// A row that DOES something in this window, as opposed to [`draw_link_row`],
/// which leaves for a URL. Gold surface, centred label, no external glyph —
/// the glyph is the whole difference the user reads.
fn draw_action_row(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    widget_id: &str,
    rect: WidgetRect,
    label: &str,
    tt: &ToolbarTheme,
) {
    let hovered = coord.is_hovered(&WidgetId::from(widget_id));
    ctx.set_fill_color(GOLD);
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    if hovered {
        // The accent is already the brightest thing in the dialog, so hover
        // reads as a veil over it rather than another colour.
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    }
    ctx.set_font("600 13px sans-serif");
    ctx.set_fill_color("#101014");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, rect.center_x(), rect.center_y());
    coord.register_on_layer(widget_id, rect, Sense::CLICK | Sense::HOVER, layer_id);
}

/// Height of the Download overlay's web (`Ready`) body: caption + primary
/// action + OTA note + the row that expands the modal, plus — only while
/// expanded — one row per entry `release_rows` returns. Shared with
/// `draw_body`'s own layout below so the frame and its content can never
/// size differently (same reason `post_block_h` is shared with the News
/// overlay above).
fn download_ready_height(expanded: bool, list_rows: usize) -> f64 {
    let collapsed = 22.0 + ROW_H + 20.0 + ROW_H;
    if !expanded || list_rows == 0 {
        collapsed
    } else {
        collapsed + 8.0 + list_rows as f64 * ROW_H
    }
}

/// Rows for the Download overlay's expanded list: every OTHER platform's
/// current build first, then every release that is not the newest for its
/// own platform ("older versions" — the visitor's own platform included).
/// The visitor's own current build is never in here — it is already the
/// primary action row above.
///
/// Relies on the server returning newest-first
/// (`GET /api/updates/list`, confirmed live 2026-09-03), so the first entry
/// seen for a given platform IS that platform's current build; no separate
/// version comparison is needed.
fn release_rows<'a>(
    own_platform: &str,
    releases: &'a [CloudReleaseEntry],
) -> Vec<&'a CloudReleaseEntry> {
    let mut seen_platform = std::collections::HashSet::new();
    let mut current = Vec::new();
    let mut older = Vec::new();
    for r in releases {
        if seen_platform.insert(r.platform.as_str()) {
            if r.platform != own_platform {
                current.push(r);
            }
        } else {
            older.push(r);
        }
    }
    // Capped, like the News overlay's own MAX_NEWS_POSTS. The modal has no
    // scrolling: every row it holds makes the frame taller, and the release
    // list is the one thing here that grows without bound — a year of patch
    // releases would push the frame past the bottom of the window with no way
    // to reach what fell off. Other platforms come first in this order, so the
    // cap eats the oldest versions, which is the right thing to lose.
    current
        .into_iter()
        .chain(older)
        .take(MAX_RELEASE_ROWS)
        .collect()
}

/// Human label for a platform id (`mlc_updater::platform`'s own ids) —
/// mirrors the landing page's own `detectPlatform()` labels
/// (`mylittlechart-landing/web/index.html`). Unrecognised ids (a future
/// platform this build doesn't know about yet) fall back to the id itself
/// rather than hiding the row.
fn platform_label(id: &str) -> &str {
    match id {
        "win64" => "Windows",
        "macos-arm64" => "macOS (Apple Silicon)",
        "macos-x64" => "macOS (Intel)",
        "linux64" => "Linux",
        other => other,
    }
}

/// `file_size` bytes as a one-decimal MB string, for the expanded release
/// rows.
fn format_file_size_mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_048_576.0)
}

/// `created_at` trimmed from its full ISO-8601 timestamp to the date —
/// this crate has no datetime dependency to parse it properly, and the
/// date is all the row needs.
fn format_release_date(created_at: &str) -> &str {
    created_at.split('T').next().unwrap_or(created_at)
}

/// The row that expands/collapses the Download overlay's release list — a
/// toggle, not a navigation, so it borrows [`draw_link_row`]'s surface but
/// swaps the trailing "opens externally" glyph for a chevron that flips
/// with the state (that glyph would be a lie here: nothing leaves the
/// window).
#[allow(clippy::too_many_arguments)]
fn draw_expand_row(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    widget_id: &str,
    rect: WidgetRect,
    label: &str,
    expanded: bool,
    tt: &ToolbarTheme,
) {
    let hovered = coord.is_hovered(&WidgetId::from(widget_id));
    ctx.set_fill_color(if hovered { &tt.item_bg_hover } else { &tt.dropdown_bg });
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);

    let icon = if expanded { Icon::ChevronUp } else { Icon::ChevronDown };
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

    coord.register_on_layer(widget_id, rect, Sense::CLICK | Sense::HOVER, layer_id);
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
        // width counts CHARS: slicing a str at a byte offset that is not a
        // char boundary panics and takes the whole wasm render loop down —
        // an em-dash in a blog summary did exactly that.
        while rest.chars().count() > width {
            let byte_cap = rest
                .char_indices()
                .nth(width)
                .map(|(i, _)| i)
                .unwrap_or(rest.len());
            let cut = rest[..byte_cap].rfind(' ').unwrap_or(byte_cap);
            lines.push(rest[..cut].trim().to_string());
            rest = rest[cut..].trim_start();
        }
        lines.push(rest.to_string());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::wrap_text;

    /// Byte-offset slicing panics inside a multibyte char (an em-dash in a
    /// blog summary crashed the whole wasm render loop). Wrapping must hold
    /// for non-ASCII text.
    #[test]
    fn wrap_handles_multibyte_chars() {
        let text = "chart.example now opens straight into the wasm terminal — the marketing page moved to /home";
        let lines = wrap_text(text, 62);
        assert!(lines.len() >= 2);
        assert_eq!(lines.concat().replace(' ', ""), text.replace(' ', ""));
    }
}
