//! News / Download overlays (cloud builds only).
//!
//! Standard modal shell (`ui::widgets::modal`) over a dimmed backdrop, one
//! dialog per [`CloudOverlayKind`]:
//! - News — the blog feed (fetched by the shell driver into
//!   [`CloudOverlayData`]), plus a LINKS section (social/project links,
//!   opened in a new tab via the `open_url_requested` mailbox) — a
//!   standalone Links overlay was thin enough content on its own that the
//!   owner folded it in here (2026-09-04) rather than keep it a chrome
//!   button of its own. The two outgrew one scroll the same day, so the
//!   list view carries the shell's own horizontal tab bar ([`NewsTab`]):
//!   `Posts`, the default glance — the LINKS section under a handful of
//!   recent editorial entries (`kind != "patch"`) — and `PatchNotes`, the
//!   full, unbounded release-note history (`kind == "patch"`) with nothing
//!   under it.
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
    NewsTab, UpdateOverlayPhase,
};
use crate::ui::scroll_widget::{ScrollableContainer, ScrollableConfig, ScrollableResult};
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{
    register_dimmer_zones, render_modal_shell, ModalPlacement, ModalShell, ModalShellSpec,
    ModalTab, ModalTabs, ModalTheme, TabFit, SETTINGS_BUTTON_RADIUS,
};
use crate::ui::widgets::types::WidgetTheme;
use crate::ui::z_order::ZLayer;
use crate::ui::Icon;

/// Accent colour shared by every overlay in this file family — also read by
/// `forum_v2_overlay`/`dm_overlay` (siblings under `layout::modals`).
pub(crate) const GOLD: &str = "#f4cd63";

const MODAL_W: f64 = 420.0;
pub(crate) const ROW_H: f64 = 40.0;
/// Gap between stacked rows.
pub(crate) const ROW_GAP: f64 = 6.0;
/// Text field height — mirrors `cloud_login.rs`'s own `FIELD_H`, used by
/// every text field in this overlay family (Forum v2's title/body/reply/
/// search, DM's draft box) so they are all the same height.
pub(crate) const FIELD_H: f64 = 32.0;
/// Gap between a Post/Cancel (or similar) button pair.
pub(crate) const ACTION_BTN_GAP: f64 = 10.0;
/// Mirrors `mlc_app::text_input::{FORUM_TITLE, FORUM_BODY, FORUM_REPLY}` —
/// this crate cannot depend on `mlc-app` (dependency direction is the other
/// way), so the literal ids are spelled here too, same split
/// `cloud_login.rs`'s own `REDEEM_FIELD` already has for `REDEEM_CODE`.
/// Forum v2's compose view reuses `FORUM_TITLE`/`FORUM_BODY`; its thread
/// view's reply box reuses `FORUM_REPLY` — same one-`TextFieldStore`-field-
/// per-purpose idiom v1 used, just pointed at the new views.
pub(crate) const FORUM_TITLE_FIELD: &str = "forum_title";
pub(crate) const FORUM_BODY_FIELD: &str = "forum_body";
pub(crate) const FORUM_REPLY_FIELD: &str = "forum_reply";
/// Forum v2's search box — new field, added to `mlc_app::text_input::
/// known_field_ids` alongside this pass.
pub(crate) const FORUM_SEARCH_FIELD: &str = "forum_search";
/// DM list view's "start a conversation" peer field (nick or public id) —
/// mirrors `mlc_app::text_input::DM_PEER`. New this pass: W2 drew no entry
/// point for `DmRequest::Open` at all (see `dm_overlay.rs`'s own
/// `draw_open_peer_row`).
pub(crate) const DM_PEER_FIELD: &str = "dm_peer";

/// Social/project links shown in the News overlay's LINKS section (id,
/// label, URL). Keep in sync with the landing footer. The click handler
/// resolves ids through this table, so a row anywhere can reuse them.
///
/// No `blog` entry: the reader landing on this section is already inside the
/// blog feed (the posts above it come straight from it), so a row pointing
/// back to itself would be dead weight.
pub const SOCIAL_LINKS: &[(&str, &str, &str)] = &[
    ("github", "GitHub", "https://github.com/knyaz-tishini/mylittlechart"),
    ("x", "X / Twitter", "https://x.com/mylittlechart"),
    ("telegram", "Telegram", "https://t.me/mylittlechart"),
    ("discord", "Discord", "https://discord.gg/mylittlechart"),
    ("donate", "Support the project", "https://chart.example/donate"),
];

/// Geometry the input side needs (header drag) plus, once a view carries a
/// real scroll container, what it needs to route wheel/scrollbar-drag input
/// through it the same way the other scrollable modals in this crate do
/// (`chart_settings`, `indicator_settings`, `primitive_settings`,
/// `user_settings`, `trading_container_modal`).
#[derive(Clone, Debug, Default)]
pub struct CloudOverlayResult {
    pub header_rect: WidgetRect,
    /// Total content height the active view actually drew — `None` for a
    /// view with no scroll container (the News reading view, and the
    /// Error/Loading status views on either kind).
    pub total_content_height: Option<f64>,
    /// The scroll container's own viewport height
    /// (`shell.content.height`). Alongside `total_content_height`, this is
    /// what `ScrollState::handle_wheel`/`handle_drag` need to clamp an
    /// offset correctly.
    pub viewport_height: Option<f64>,
    /// The unclipped scroll viewport rect, registered under
    /// `cloud_overlay:scroll_viewport` with `Sense::SCROLL` — mirrors
    /// `chart_settings:scroll_viewport` and the other modals that already
    /// route wheel input through that convention
    /// (`mlc-app/src/input/mod.rs::on_scroll`). Wiring THIS overlay into
    /// that match, and into the scrollbar-drag handlers, is a follow-up
    /// outside this crate's own file scope — not done in this pass.
    pub scroll_viewport_rect: Option<WidgetRect>,
    /// Scrollbar handle rect, for drag hit-testing (same wiring gap as
    /// `scroll_viewport_rect`).
    pub scrollbar_handle_rect: Option<WidgetRect>,
    /// Scrollbar track rect, for click-to-jump hit-testing.
    pub scrollbar_track_rect: Option<WidgetRect>,
    /// Forum compose view's title field geometry — `None` unless that view
    /// is showing. Same "every modal field needs its own arm" contract
    /// `cloud_login.rs`'s `redeem_input_rect` documents: `mlc-app`'s
    /// `on_drag_start`/per-frame `update_field` passes read this to give
    /// `TextFieldStore` a hit rect and char-boundary positions.
    pub forum_title_rect: Option<WidgetRect>,
    pub forum_title_char_positions: Vec<f64>,
    /// Forum compose view's body field geometry — same contract as
    /// `forum_title_rect`.
    pub forum_body_rect: Option<WidgetRect>,
    pub forum_body_char_positions: Vec<f64>,
    /// Forum thread view's reply field geometry — same contract as
    /// `forum_title_rect`.
    pub forum_reply_rect: Option<WidgetRect>,
    pub forum_reply_char_positions: Vec<f64>,
    /// Forum v2 Boards view's search box geometry — same contract as
    /// `forum_title_rect` above. Backs `mlc_app::text_input::FORUM_SEARCH`
    /// (new this pass); full `mlc-app` click/drag wiring for it is a
    /// follow-up outside this pass, same gap `scroll_viewport_rect`'s own
    /// doc already names for the scrollbar.
    pub forum_search_rect: Option<WidgetRect>,
    pub forum_search_char_positions: Vec<f64>,
    /// DM Conversation view's draft box geometry — same contract as
    /// `forum_search_rect`. Backs `mlc_app::text_input::DM_DRAFT` (new this
    /// pass).
    pub dm_draft_rect: Option<WidgetRect>,
    pub dm_draft_char_positions: Vec<f64>,
    /// DM List view's "start a conversation" peer field geometry — same
    /// contract as `forum_search_rect` above. Backs `mlc_app::text_input::
    /// DM_PEER` (new this pass, see `dm_overlay.rs`'s `draw_open_peer_row`).
    pub dm_peer_rect: Option<WidgetRect>,
    pub dm_peer_char_positions: Vec<f64>,
}

fn title_for(state: &CloudOverlayState) -> &'static str {
    match state.kind {
        CloudOverlayKind::News => "What's new",
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

/// Which of the two News list tabs a post belongs under.
fn is_patch(post: &CloudNewsPost) -> bool {
    post.kind == "patch"
}

/// The title text drawn for `post` — its release version when it is a patch
/// note that carries one, its own title otherwise. Shared by the block and
/// the reading view so neither can show a different heading for the same
/// entry.
fn display_title(post: &CloudNewsPost) -> &str {
    if is_patch(post) {
        if let Some(v) = post.version.as_deref().filter(|v| !v.is_empty()) {
            return v;
        }
    }
    &post.title
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

/// The News overlay shows at most this many entries on the `Posts` tab —
/// the glance a reopened panel always comes back to. The `PatchNotes` tab is
/// unbounded (`news_list_h`) and is exactly what the scroll container
/// below exists for.
const MAX_NEWS_POSTS: usize = 3;

/// Gap between the last post block (or, with no posts, the list's top) and
/// the LINKS heading below it.
const LINKS_SECTION_GAP: f64 = 8.0;
/// Height of the LINKS heading line itself — the same offset cloud_login's
/// own "SIGNED IN AS" leaves before its first line of content, so the two
/// modals' section headers read as one family.
const LINKS_HEADING_H: f64 = 22.0;

/// Height of the LINKS section the News list appends below its posts: gap +
/// heading + one row per [`SOCIAL_LINKS`] entry. A function, not inlined at
/// both call sites, so `content_height` and `draw_links_section` can never
/// disagree on the frame this section needs — same reason `post_block_h`
/// mirrors `draw_news_post`.
fn links_section_h() -> f64 {
    LINKS_SECTION_GAP + LINKS_HEADING_H + SOCIAL_LINKS.len() as f64 * ROW_H
}

/// The News overlay's list-view tabs — `Posts` (the default glance: up to
/// `MAX_NEWS_POSTS` editorial entries + LINKS), `PatchNotes` (the full
/// release-note history, no LINKS), `Forum` and `Messages` (both paid-only,
/// see `NEWS_TAB_ITEMS_ALL`/`news_tab_items` further down). Drawn only over
/// the list view: the in-modal reading view (`state.selected_post.is_some()`)
/// and the Download overlay have nothing to switch between, so neither
/// carries a tab bar at all.

/// Height of the News list view for `tab` — the block sum for whichever
/// slice of `posts` that tab shows (`Posts` = `kind != "patch"`, capped at
/// `MAX_NEWS_POSTS`; `PatchNotes` = `kind == "patch"`, unbounded), plus the
/// LINKS section only on `NewsTab::Posts`. A function, not inlined at
/// `content_height` and `draw_news_list` both, for the same reason
/// `post_block_h` is shared with `draw_news_post`: the frame and what
/// actually gets painted into it can never disagree.
fn news_list_h(tab: NewsTab, posts: &[CloudNewsPost]) -> f64 {
    match tab {
        NewsTab::Posts => {
            let blocks: f64 = posts
                .iter()
                .filter(|p| !is_patch(p))
                .take(MAX_NEWS_POSTS)
                .map(post_block_h)
                .sum();
            blocks + links_section_h()
        }
        NewsTab::PatchNotes => posts.iter().filter(|p| is_patch(p)).map(post_block_h).sum(),
        // Never actually reached — `content_height`/`draw_body` branch off
        // to `forum_v2_overlay`/`dm_overlay` before either call site here
        // runs. Kept exhaustive rather than `unreachable!()` so a future
        // caller of this function cannot panic by forgetting that split.
        NewsTab::Forum | NewsTab::Messages => 0.0,
    }
}

/// Height of the Download overlay's Error/Loading status views — headline
/// plus, on Error, one clipped detail line (`draw_status`). Named so
/// `content_height` and `draw_body`'s own scroll-container sizing read the
/// same number instead of repeating the arithmetic twice.
const DOWNLOAD_STATUS_H: f64 = 22.0 + ROW_H + 24.0;

/// Ceiling on the Download body once it is wrapped in a scroll container.
/// The collapsed view is ~122px and never needs it; the number matters
/// only once the release catalogue is expanded, where it leaves room for
/// the collapsed rows plus roughly six release rows (`6.0 * ROW_H`) before
/// the container starts scrolling — enough to glance at a short history
/// without a scrollbar, small enough that the modal never grows past what
/// a laptop-sized window has room for under the toolbar button it opens
/// from.
const DOWNLOAD_MAX_BODY_H: f64 = 400.0;

/// Ceiling on the News list body once it is wrapped in a scroll container.
/// Sized so the default `Posts` tab (up to `MAX_NEWS_POSTS` two-line entries
/// plus the fixed LINKS section, ~470px worst case) fits without ever
/// needing to scroll in the common case; the `PatchNotes` tab, whose entry
/// count is unbounded, is what actually exercises the scroll container this leaves
/// room for.
const NEWS_LIST_MAX_BODY_H: f64 = 480.0;

/// Height of the Download overlay's Update (desktop) body for `phase` —
/// shared between `content_height` and `draw_body`'s own scroll-container
/// sizing so the frame and what `draw_update_phase` actually paints can
/// never disagree (same doctrine as `post_block_h` paired with
/// `draw_news_post`).
fn update_phase_h(phase: &UpdateOverlayPhase) -> f64 {
    match phase {
        UpdateOverlayPhase::Available { notes, .. } => {
            22.0 + wrap_text(notes, 62).len().min(4) as f64 * 16.0 + 10.0 + ROW_H
        }
        UpdateOverlayPhase::Checking | UpdateOverlayPhase::UpToDate { .. } => 22.0 + 24.0,
        UpdateOverlayPhase::RestartPending => 22.0 + ROW_H + 8.0,
        UpdateOverlayPhase::Failed { .. } => 22.0 + 24.0 + ROW_H,
        _ => 22.0 + 24.0,
    }
}

/// Body height per kind — the shell sizes the frame from this. For the two
/// views a scroll container wraps (the Download body, the News list view)
/// this is the FULL content height, uncapped — `render_cloud_overlay` is
/// what clamps the frame at `DOWNLOAD_MAX_BODY_H`/`NEWS_LIST_MAX_BODY_H`,
/// and `draw_body`'s own `ScrollableContainer` is what makes the excess
/// reachable rather than lost.
fn content_height(state: &CloudOverlayState) -> f64 {
    match state.kind {
        CloudOverlayKind::Download => match &state.data {
            CloudOverlayData::Update { phase } => update_phase_h(phase),
            // Web view: caption + primary action + OTA note + the row that
            // expands the modal, plus — only while expanded — the grouped
            // release catalogue. Shared arithmetic with `draw_ready_view`
            // via `download_ready_height`/`release_rows`/`group_release_rows`
            // so the frame can never size differently from what actually
            // gets drawn into it.
            CloudOverlayData::Ready { releases, .. } => download_ready_height(
                state.expanded,
                &group_release_rows(&release_rows(releases)),
            ),
            _ => DOWNLOAD_STATUS_H,
        },
        CloudOverlayKind::News if state.news_tab == NewsTab::Forum => super::forum_v2_overlay::height(state),
        CloudOverlayKind::News if state.news_tab == NewsTab::Messages => super::dm_overlay::height(state),
        CloudOverlayKind::News => match &state.data {
            CloudOverlayData::News { posts } => {
                // Reading view: one post, expanded — no tabs over it (the
                // shell only sizes in the tab bar when `render_cloud_overlay`
                // actually gives it one). List view: whichever of the two
                // tabs is active, via `news_list_h`.
                if let Some(post) = state.selected_post.and_then(|i| posts.get(i)) {
                    post_detail_h(post)
                } else {
                    news_list_h(state.news_tab, posts)
                }
            }
            _ => 84.0,
        },
    }
}

/// Render the cloud overlay (News, which carries the LINKS section, Forum
/// and Messages tabs, and Download). `current_time_ms` drives the Forum/
/// Messages tabs' relative timestamps and their text fields' caret blink —
/// the News/Download kinds never needed a clock before this (unlike
/// `render_cloud_login_modal`, which has taken one since the gift-code
/// field).
pub fn render_cloud_overlay(
    ctx: &mut dyn RenderContext,
    window_w: f64,
    window_h: f64,
    state: &CloudOverlayState,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    current_time_ms: u64,
    input_coordinator: &mut InputCoordinator,
) -> CloudOverlayResult {
    let theme = ModalTheme::from_toolbar(
        &frame_theme.modal_bg,
        &toolbar_theme.separator,
        toolbar_theme,
    );
    let mut spec = ModalShellSpec {
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
    // Tabs only over the News list view (incl. the Forum/Messages tabs) — a
    // reading view or the Download overlay has nothing to switch between,
    // so `spec.tabs` stays `None` and the shell draws no tab bar (and
    // charges the frame no `TAB_BAR_H`) for either of them. Forum/Messages
    // are gated by `forum_entitled` OR already being the active tab — kept
    // shown once selected even if entitlement is re-asked mid-session and
    // (implausibly) comes back false, rather than yanking the tab bar out
    // from under an open thread/conversation.
    let on_paid_tab = matches!(state.news_tab, NewsTab::Forum | NewsTab::Messages);
    let show_news_tabs = state.kind == CloudOverlayKind::News
        && state.selected_post.is_none()
        && (matches!(&state.data, CloudOverlayData::News { .. }) || on_paid_tab);
    if show_news_tabs {
        let items = news_tab_items(state.forum_entitled || on_paid_tab);
        spec = spec.with_tabs(ModalTabs::Horizontal {
            items,
            active: state.news_tab.index(),
            fit: TabFit::Equal,
        });
        // The News list view is the one whose content is unbounded (the
        // `PatchNotes` tab, a long history under `Posts`'s LINKS section,
        // the Forum board/thread list/thread body, and the DM conversation
        // list/history) — cap it here and let `draw_body`'s scroll
        // container reach the rest.
        spec = spec.with_max_content_height(NEWS_LIST_MAX_BODY_H);
    }
    // The Download body is the OTHER unbounded view (the release catalogue
    // once expanded). Capping every `Download` data variant, not just
    // `Ready`, is what lets `draw_body` wrap the whole kind in one scroll
    // container instead of special-casing which variant needs it.
    if state.kind == CloudOverlayKind::Download {
        spec = spec.with_max_content_height(DOWNLOAD_MAX_BODY_H);
    }
    let mut result = CloudOverlayResult::default();
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
            draw_body(
                ctx,
                coord,
                layer_id,
                shell,
                state,
                frame_theme,
                toolbar_theme,
                current_time_ms,
                &mut result,
            );
        },
    );
    result.header_rect = shell.header;
    result
}

/// The News overlay's tab strip: `Posts`/`PatchNotes` always, `Forum` and
/// `Messages` only when `with_forum` (entitlement, or either one already
/// being the active tab — see `render_cloud_overlay`'s own call site). A
/// fixed 4-slot array sliced down rather than separate `const`s, so the
/// first two entries can never drift between the gated and ungated form.
const NEWS_TAB_ITEMS_ALL: [ModalTab<'static>; 4] = [
    ModalTab { id: "posts", label: "Posts", icon: None },
    ModalTab { id: "patch_notes", label: "Patch notes", icon: None },
    ModalTab { id: "forum", label: "Forum", icon: None },
    ModalTab { id: "messages", label: "Messages", icon: None },
];

fn news_tab_items(with_forum: bool) -> &'static [ModalTab<'static>] {
    if with_forum {
        &NEWS_TAB_ITEMS_ALL
    } else {
        &NEWS_TAB_ITEMS_ALL[..2]
    }
}

/// Scrollbar geometry shared by every `ScrollableContainer` in this file —
/// same 8px handle + 4px padding, auto-hidden while content fits, that
/// `chart_settings.rs` uses for its own tab bodies. Also used by
/// `forum_v2_overlay`/`dm_overlay`'s own scroll containers.
pub(crate) fn scroll_config() -> ScrollableConfig {
    ScrollableConfig {
        scrollbar_width: 8.0,
        scrollbar_padding: 4.0,
        always_show_scrollbar: false,
    }
}

/// Register the scrollbar handle (inflated ±5px on X, for a forgiving drag
/// target) and track (for a jump-to-position click) — the same wiring every
/// other scrollable modal in this crate uses (`chart_settings`,
/// `user_settings`, `trading_container_modal`, ...). One
/// `cloud_overlay:scrollbar_*` id pair regardless of which view drew it,
/// since only one is ever on screen at a time. Also used by
/// `forum_v2_overlay`/`dm_overlay`'s own scroll containers.
pub(crate) fn register_scrollbar_hit_zones(
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    scroll_result: &ScrollableResult,
) {
    if let Some(ref hr) = scroll_result.handle_rect {
        let inflated = WidgetRect::new(hr.x - 5.0, hr.y, hr.width + 10.0, hr.height);
        coord.register_on_layer("cloud_overlay:scrollbar_handle", inflated, Sense::DRAG, layer_id);
    }
    if let Some(ref tr) = scroll_result.track_rect {
        coord.register_on_layer(
            "cloud_overlay:scrollbar_track",
            WidgetRect::new(tr.x, tr.y, tr.width, tr.height),
            Sense::CLICK,
            layer_id,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_body(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    shell: &ModalShell,
    state: &CloudOverlayState,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    result: &mut CloudOverlayResult,
) {
    let x = shell.content.x;
    let y = shell.content.y;
    let w = shell.content.width;
    let viewport_h = shell.content.height;

    match state.kind {
        CloudOverlayKind::Download => {
            let drawn_h = match &state.data {
                CloudOverlayData::Update { phase } => update_phase_h(phase),
                CloudOverlayData::Ready { releases, .. } => download_ready_height(
                    state.expanded,
                    &group_release_rows(&release_rows(releases)),
                ),
                _ => DOWNLOAD_STATUS_H,
            };

            // Every Download variant paints inside ONE scroll container: the
            // clip it applies is what actually guarantees nothing paints
            // outside the shell's content rect (the OTA/update copy was
            // spilling past the frame's right edge and bottom before this),
            // and the same container is what lets the expanded release
            // catalogue scroll once it outgrows `DOWNLOAD_MAX_BODY_H`.
            let viewport_rect = WidgetRect::new(x, y, w, viewport_h);
            coord.register_on_layer(
                "cloud_overlay:scroll_viewport",
                viewport_rect,
                Sense::SCROLL,
                layer_id,
            );
            let scrollable = ScrollableContainer::new(viewport_rect, &state.scroll, Some(scroll_config()));
            scrollable.begin(ctx);
            let sx = x;
            let sy = scrollable.content_y();
            // From here the paintable width is `content_width()`, not `w` —
            // the container clips to it, and a row laid out to the full
            // viewport width loses its right end at that clip.
            let sw = scrollable.content_width();

            match &state.data {
                CloudOverlayData::Update { phase } => {
                    draw_update_phase(ctx, coord, layer_id, sx, sy, sw, phase, tt);
                }
                CloudOverlayData::Ready { platform, version, releases, .. } => {
                    draw_ready_view(
                        ctx, coord, layer_id, sx, sy, sw, state.expanded, platform, version, releases, tt,
                    );
                }
                CloudOverlayData::Error { message } => {
                    draw_status(ctx, tt, sx, sy, sw, "Failed to load release info", Some(message));
                }
                _ => {
                    draw_status(ctx, tt, sx, sy, sw, "Loading release info…", None);
                }
            }

            let widget_theme = WidgetTheme::default();
            let scroll_result = scrollable.end(ctx, drawn_h, &widget_theme);
            result.total_content_height = Some(drawn_h);
            result.viewport_height = Some(viewport_h);
            result.scroll_viewport_rect = Some(viewport_rect);
            result.scrollbar_handle_rect = scroll_result.handle_rect;
            result.scrollbar_track_rect = scroll_result.track_rect;
            register_scrollbar_hit_zones(coord, layer_id, &scroll_result);
        }

        CloudOverlayKind::News if state.news_tab == NewsTab::Forum => {
            super::forum_v2_overlay::draw(ctx, coord, layer_id, x, y, w, viewport_h, state, ft, tt, current_time_ms, result);
        }

        CloudOverlayKind::News if state.news_tab == NewsTab::Messages => {
            super::dm_overlay::draw(ctx, coord, layer_id, x, y, w, viewport_h, state, ft, tt, current_time_ms, result);
        }

        CloudOverlayKind::News => match &state.data {
            CloudOverlayData::News { posts } => {
                if let Some(post) = state.selected_post.and_then(|i| posts.get(i)) {
                    // The reading view is one post — never long enough on
                    // its own (owner defect list names only the catalogue
                    // and the `All` tab) to need a second scroll container;
                    // `content_height` sizes the frame to it directly, same
                    // as before this pass.
                    draw_news_detail(ctx, coord, layer_id, x, y, w, post, tt);
                } else {
                    let drawn_h = news_list_h(state.news_tab, posts);
                    let viewport_rect = WidgetRect::new(x, y, w, viewport_h);
                    coord.register_on_layer(
                        "cloud_overlay:scroll_viewport",
                        viewport_rect,
                        Sense::SCROLL,
                        layer_id,
                    );
                    let scrollable =
                        ScrollableContainer::new(viewport_rect, &state.scroll, Some(scroll_config()));
                    scrollable.begin(ctx);
                    let sy = scrollable.content_y();
                    let sw = scrollable.content_width();
                    draw_news_list(ctx, coord, layer_id, x, sy, sw, sw, state.news_tab, posts, tt);

                    let widget_theme = WidgetTheme::default();
                    let scroll_result = scrollable.end(ctx, drawn_h, &widget_theme);
                    result.total_content_height = Some(drawn_h);
                    result.viewport_height = Some(viewport_h);
                    result.scroll_viewport_rect = Some(viewport_rect);
                    result.scrollbar_handle_rect = scroll_result.handle_rect;
                    result.scrollbar_track_rect = scroll_result.track_rect;
                    register_scrollbar_hit_zones(coord, layer_id, &scroll_result);
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

/// The News overlay's list view: `NewsTab::Posts` draws up to
/// `MAX_NEWS_POSTS` editorial blocks (`kind != "patch"`) and the LINKS
/// section beneath them — the glance a reopened panel always comes back to;
/// `NewsTab::PatchNotes` draws every release note (`kind == "patch"`),
/// newest first, and stops there, since a reader who asked for the release
/// history did not come here for the LINKS section. Widget ids for the post
/// rows stay `cloud_overlay:post:{i}` with `i` the row's own index into the
/// UNFILTERED `posts` slice (not the position within the filtered/capped
/// list this draws), so `panel_click.rs`'s `posts.get(i)` lookup for the
/// reading view resolves to the same entry regardless of which tab drew the
/// row that opened it. Called with `y` already at the scroll container's
/// own offset content position (`ScrollableContainer::content_y()`), so
/// every row's hit zone registers at the same on-screen position it paints
/// at.
#[allow(clippy::too_many_arguments)]
fn draw_news_list(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    row_w: f64,
    tab: NewsTab,
    posts: &[CloudNewsPost],
    tt: &ToolbarTheme,
) {
    let mut py = y;
    match tab {
        NewsTab::Posts => {
            let shown = posts
                .iter()
                .enumerate()
                .filter(|(_, p)| !is_patch(p))
                .take(MAX_NEWS_POSTS);
            for (i, post) in shown {
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
            draw_links_section(ctx, coord, layer_id, x, py, row_w, tt);
        }
        NewsTab::PatchNotes => {
            let shown = posts.iter().enumerate().filter(|(_, p)| is_patch(p));
            for (i, post) in shown {
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
        }
        // Never reached — `draw_body` branches to `forum_v2_overlay`/
        // `dm_overlay` before this function is ever called with either tab.
        // Kept exhaustive rather than `unreachable!()`, same reasoning as
        // `news_list_h`'s own arm above.
        NewsTab::Forum | NewsTab::Messages => {}
    }
}

/// One blog post block in the News overlay: date, title (or version, via
/// [`display_title`], for a patch note that carries one), summary — the
/// whole block is a click zone out to the post. Returns the height consumed.
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
    ctx.fill_text(display_title(post), x + 8.0, y + 22.0);

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

/// LINKS section appended below the News list's posts: a small-caps heading
/// in the same font/colour cloud_login's own "SIGNED IN AS"/"PLAN" headers
/// use (so the two modals read as one family), followed by one row per
/// [`SOCIAL_LINKS`] entry. Widget ids stay `cloud_overlay:link:{id}` — the
/// input side (`panel_click.rs`) resolves them through `SOCIAL_LINKS`
/// regardless of which overlay drew the row, so folding the old standalone
/// Links overlay in here needed no change on that side.
fn draw_links_section(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    row_w: f64,
    tt: &ToolbarTheme,
) {
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.set_font("11px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.fill_text("LINKS", x, y + LINKS_SECTION_GAP);

    let mut ry = y + LINKS_SECTION_GAP + LINKS_HEADING_H;
    for (id, label, _url) in SOCIAL_LINKS {
        draw_link_row(
            ctx,
            coord,
            layer_id,
            &format!("cloud_overlay:link:{}", id),
            WidgetRect::new(x, ry, row_w, ROW_H - ROW_GAP),
            label,
            Icon::Globe,
            tt,
        );
        ry += ROW_H;
    }
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

// =============================================================================
// Forum tab (News overlay, `NewsTab::Forum`, paid-only) — three sub-views:
// list, one open thread, compose. Content lives in `CloudOverlayState::
// forum_list`/`forum_view`, not `CloudOverlayState::data` — see those
// fields' own doc for why.
// =============================================================================

/// Days-since-epoch via Howard Hinnant's `days_from_civil` algorithm
/// (proleptic Gregorian) — the only date arithmetic this crate needs, and
/// pulling in a datetime crate for one field (`relative_time` below) is not
/// worth it. Returns `None` on anything shorter than `"YYYY-MM-DDTHH:MM:SS"`
/// or with a non-numeric component; a malformed timestamp then degrades to
/// the plain trimmed-date fallback in `relative_time`, never a wrong label.
fn parse_rfc3339_unix_secs(s: &str) -> Option<i64> {
    if s.len() < 19 {
        return None;
    }
    let year: i64 = s.get(0..4)?.parse().ok()?;
    let month: i64 = s.get(5..7)?.parse().ok()?;
    let day: i64 = s.get(8..10)?.parse().ok()?;
    let hour: i64 = s.get(11..13)?.parse().ok()?;
    let min: i64 = s.get(14..16)?.parse().ok()?;
    let sec: i64 = s.get(17..19)?.parse().ok()?;
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (month + 9) % 12; // [0, 11], Mar = 0
    let doy = (153 * mp + 2) / 5 + day - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    let days_since_epoch = era * 146097 + doe - 719468;
    Some(days_since_epoch * 86400 + hour * 3600 + min * 60 + sec)
}

/// `"just now"` / `"5m ago"` / `"3h ago"` / `"2d ago"` / the trimmed date
/// past 30 days — `now_ms` is `current_time_ms`, the same clock every other
/// overlay timestamp compares against. Also used by `forum_v2_overlay`/
/// `dm_overlay` for their own timestamps.
pub(crate) fn relative_time(iso: &str, now_ms: u64) -> String {
    let fallback = || iso.split('T').next().unwrap_or(iso).to_string();
    let Some(secs) = parse_rfc3339_unix_secs(iso) else {
        return fallback();
    };
    let now_secs = (now_ms / 1000) as i64;
    let delta = (now_secs - secs).max(0);
    match delta {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", delta / 60),
        3600..=86399 => format!("{}h ago", delta / 3600),
        86400..=2591999 => format!("{}d ago", delta / 86400),
        _ => fallback(),
    }
}

/// The Download overlay on a desktop build: the in-process updater's state.
///
/// Only two phases carry an action. `Available` offers Install, which posts
/// the `update_install_requested` mailbox for the shell to turn into an
/// updater command; `Failed` offers a retry through the same mailbox, since
/// re-running the install IS the retry. Everything between them is the
/// updater working on its own — a progress line, nothing to press.
///
/// Every line is run through `fit_text_to_width`: this copy is fixed English
/// text, but "fixed" never meant "short enough in every font the browser
/// actually renders it in" — the OTA/update wording was spilling past the
/// frame's right edge before this, and clipping it defensively (rather than
/// trusting the string) is what `draw_status`'s error detail already did.
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
            let line = crate::ui::widgets::fit_text_to_width(ctx, "Checking for updates…", row_w);
            ctx.fill_text(&line, x, y);
        }

        UpdateOverlayPhase::UpToDate { version } => {
            let line = crate::ui::widgets::fit_text_to_width(
                ctx,
                &format!("You are on the latest release, v{version}."),
                row_w,
            );
            ctx.fill_text(&line, x, y);
        }

        UpdateOverlayPhase::Available { version, notes } => {
            let headline = crate::ui::widgets::fit_text_to_width(
                ctx,
                &format!("Version {version} is available."),
                row_w,
            );
            ctx.fill_text(&headline, x, y);
            let mut ny = y + 22.0;
            ctx.set_font("12px sans-serif");
            ctx.set_fill_color(&tt.item_text);
            for line in wrap_text(notes, 62).into_iter().take(4) {
                let line = crate::ui::widgets::fit_text_to_width(ctx, &line, row_w);
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
            let line = crate::ui::widgets::fit_text_to_width(
                ctx,
                &format!("Downloading… {percent}%"),
                row_w,
            );
            ctx.fill_text(&line, x, y);
            // A bar rather than a spinner: this is the one phase whose length
            // the updater actually knows.
            let track_y = y + 26.0;
            ctx.set_fill_color(&tt.dropdown_bg);
            ctx.fill_rounded_rect(x, track_y, row_w, 6.0, 3.0);
            ctx.set_fill_color(GOLD);
            ctx.fill_rounded_rect(x, track_y, row_w * (*percent as f64 / 100.0), 6.0, 3.0);
        }

        UpdateOverlayPhase::Verifying => {
            let line = crate::ui::widgets::fit_text_to_width(ctx, "Verifying the download…", row_w);
            ctx.fill_text(&line, x, y);
        }

        UpdateOverlayPhase::Installing => {
            let line = crate::ui::widgets::fit_text_to_width(ctx, "Installing…", row_w);
            ctx.fill_text(&line, x, y);
        }

        UpdateOverlayPhase::RestartPending => {
            let line = crate::ui::widgets::fit_text_to_width(
                ctx,
                "Installed. It takes effect the next time you start the app.",
                row_w,
            );
            ctx.fill_text(&line, x, y);
        }

        UpdateOverlayPhase::Failed { message } => {
            ctx.set_fill_color(&tt.item_text);
            let headline = crate::ui::widgets::fit_text_to_width(ctx, "The update did not complete.", row_w);
            ctx.fill_text(&headline, x, y);
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
/// the glyph is the whole difference the user reads. Also used by
/// `forum_v2_overlay`/`dm_overlay`.
pub(crate) fn draw_action_row(
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

/// One version's block in the Download overlay's expanded release
/// catalogue: the version string plus every entry (platform) published
/// under it, in the same relative order `release_rows` already
/// established.
struct ReleaseVersionGroup<'a> {
    version: &'a str,
    entries: Vec<&'a CloudReleaseEntry>,
}

/// Vertical gap a group header band leaves above and below its label line,
/// so blocks read as evenly spaced rather than the band crowding one side.
const VERSION_SEP_GAP: f64 = 8.0;
/// Height of the version label line inside a group header band (11px font
/// plus slack). EVERY group gets the band above it — first included: the
/// band carries the block's version and date on the left, a hairline
/// filling the rest, so no block in the catalogue is an unlabeled orphan
/// (owner, 2026-09-04).
const VERSION_SEP_LABEL_H: f64 = 14.0;
/// Total vertical space one group header band eats above its release block —
/// `download_ready_height`'s and `draw_ready_view`'s shared unit, so the two
/// can never disagree on how tall the expanded list actually is.
const VERSION_SEP_H: f64 = VERSION_SEP_GAP + VERSION_SEP_LABEL_H + VERSION_SEP_GAP;

/// Height of the Download overlay's web (`Ready`) body: caption + primary
/// action + OTA note + the row that expands the modal, plus — only while
/// expanded — one row per entry across every version group in `groups`,
/// plus one `VERSION_SEP_H` header band per group (every group is headed,
/// the first one too). Shared with `draw_ready_view`'s own layout
/// below so the frame and its content can never size differently (same
/// reason `post_block_h` is shared with the News overlay above).
fn download_ready_height(expanded: bool, groups: &[ReleaseVersionGroup<'_>]) -> f64 {
    let collapsed = 22.0 + ROW_H + 20.0 + ROW_H;
    if !expanded || groups.is_empty() {
        return collapsed;
    }
    let rows: usize = groups.iter().map(|g| g.entries.len()).sum();
    collapsed + 8.0 + rows as f64 * ROW_H + groups.len() as f64 * VERSION_SEP_H
}

/// Rows for the Download overlay's expanded list: every platform's current
/// build first, then every release that is not the newest for its own
/// platform ("older versions"). The visitor's own platform is NOT excluded:
/// its current build also sits in the primary action row above, but the
/// owner ruled (2026-09-04) the catalogue lists the complete wave — win,
/// both macs, linux — not the wave minus the reader.
///
/// Relies on the server returning newest-first
/// (`GET /api/updates/list`, confirmed live 2026-09-03), so the first entry
/// seen for a given platform IS that platform's current build; no separate
/// version comparison is needed.
///
/// Unbounded — this used to cap itself at a fixed row count because the
/// modal had no scrolling, and every row past that count fell off with no
/// way to reach it. Now that `draw_body` wraps the Download view in a real
/// `ScrollableContainer` (2026-09-04), the whole history is reachable and
/// the cap is gone; `group_release_rows` + `draw_ready_view`'s header bands
/// are what keep a long list readable instead.
fn release_rows(releases: &[CloudReleaseEntry]) -> Vec<&CloudReleaseEntry> {
    let mut seen_platform = std::collections::HashSet::new();
    let mut current = Vec::new();
    let mut older = Vec::new();
    for r in releases {
        if seen_platform.insert(r.platform.as_str()) {
            current.push(r);
        } else {
            older.push(r);
        }
    }
    current.into_iter().chain(older).collect()
}

/// Group `rows` by version, folding every ADJACENT row that shares one into
/// a single block — never a global regroup, since `release_rows` already
/// orders current builds before older ones and the same version string
/// could in principle recur on both sides of that boundary; regrouping
/// those together would silently reorder history. The project's own
/// release convention (`nemo/mylittlechart/CLAUDE.md`: one version ships to
/// every platform in the same wave) means the common case is exactly what
/// the owner asked for (2026-09-04) — one block per wave, every platform
/// under it. Inside a block the platforms are sorted alphabetically by
/// label (Linux → macOS → Windows): the server order within a wave is just
/// publish order, which has no meaning to a reader (owner, 2026-09-05).
fn group_release_rows<'a>(rows: &[&'a CloudReleaseEntry]) -> Vec<ReleaseVersionGroup<'a>> {
    let mut groups: Vec<ReleaseVersionGroup<'a>> = Vec::new();
    for r in rows {
        match groups.last_mut() {
            Some(last) if last.version == r.version.as_str() => last.entries.push(r),
            _ => groups.push(ReleaseVersionGroup { version: r.version.as_str(), entries: vec![*r] }),
        }
    }
    for g in &mut groups {
        g.entries.sort_by(|a, b| platform_sort_key(&a.platform).cmp(&platform_sort_key(&b.platform)));
    }
    groups
}

/// Left inset before a release row's label — mirrors `draw_link_row`'s own
/// `rect.x + 38.0` for its label start, so the truncation budget matches
/// exactly what the row actually paints into.
const RELEASE_ROW_LABEL_X: f64 = 38.0;
/// Reserved on the right for `draw_link_row`'s trailing "opens externally"
/// glyph (a 14px icon at `rect.right() - 26.0`) plus a little breathing
/// room, so a full-width label never runs under it.
const RELEASE_ROW_LABEL_TRAILING: f64 = 32.0;

/// A release row's label: platform + size, nothing else. The version and
/// the date live in the group header band above the block — repeating them
/// in every row is noise under a labelled group (owner, 2026-09-04). Falls
/// back to `fit_text_to_width` if even platform + size overflows, so
/// nothing ever paints past `max_w`. Matches `draw_link_row`'s own
/// `"13px sans-serif"` font so the measurement is of what actually gets
/// drawn.
fn release_row_label(
    ctx: &mut dyn RenderContext,
    platform_label: &str,
    size: &str,
    max_w: f64,
) -> String {
    ctx.set_font("13px sans-serif");
    let full = format!("{} — {}", platform_label, size);
    if ctx.measure_text(&full) <= max_w {
        return full;
    }
    crate::ui::widgets::fit_text_to_width(ctx, &full, max_w)
}

/// The Download overlay's web (`Ready`) body: caption, primary action, OTA
/// note, the row that expands the modal, and — while expanded — the
/// release catalogue grouped into one block per version (owner ask,
/// 2026-09-04) with a labelled header band above EVERY block — version and
/// date on the left, hairline right; rows under it carry platform and size
/// only. Height is
/// `download_ready_height`'s job, not this function's — the two must
/// never disagree, so this never recomputes what that already owns.
#[allow(clippy::too_many_arguments)]
fn draw_ready_view(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    expanded: bool,
    platform: &str,
    version: &str,
    releases: &[CloudReleaseEntry],
    tt: &ToolbarTheme,
) {
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    let cap = format!("{} — latest release v{}", platform_label(platform), version);
    // Built from a server-supplied label with no length guarantee — clip it
    // the same way `draw_status`'s error detail does, rather than let a
    // long platform name run past the frame's right edge.
    let cap = crate::ui::widgets::fit_text_to_width(ctx, &cap, w);
    ctx.fill_text(&cap, x, y);

    // Primary action: THIS visitor's own build, straight away — no
    // dropdown, no picking a platform first.
    draw_action_row(
        ctx,
        coord,
        layer_id,
        "cloud_overlay:download:primary",
        WidgetRect::new(x, y + 22.0, w, ROW_H - ROW_GAP),
        &format!("Download for {}", platform_label(platform)),
        tt,
    );

    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&tt.item_text_muted);
    let note_y = y + 22.0 + ROW_H + 6.0;
    // Fixed copy, but "fixed" never meant "short enough in every font the
    // browser actually renders it in" — clip defensively instead of
    // trusting the string (this line was the one spilling past the frame's
    // right edge on the live wasm build).
    let note = crate::ui::widgets::fit_text_to_width(
        ctx,
        "The desktop app updates itself (OTA) once installed.",
        w,
    );
    ctx.fill_text(&note, x, note_y);

    // The row that expands the modal into the full catalogue — every other
    // platform's current build, then older versions.
    let toggle_y = note_y + 14.0;
    let toggle_label = if expanded {
        "Hide other platforms & versions"
    } else {
        "Other platforms & versions"
    };
    draw_expand_row(
        ctx,
        coord,
        layer_id,
        "cloud_overlay:toggle_expand",
        WidgetRect::new(x, toggle_y, w, ROW_H - ROW_GAP),
        toggle_label,
        expanded,
        tt,
    );

    if !expanded {
        return;
    }
    let groups = group_release_rows(&release_rows(releases));
    if groups.is_empty() {
        return;
    }
    let label_max_w = (w - RELEASE_ROW_LABEL_X - RELEASE_ROW_LABEL_TRAILING).max(0.0);
    let mut ry = toggle_y + ROW_H + 8.0;
    for group in groups.iter() {
        // A header band above EVERY version block, the first included — an
        // unlabeled first block read as an orphan next to the labelled
        // older ones (owner, 2026-09-04). Version + date on the left,
        // hairline filling the rest of the line; the rows below then carry
        // only platform and size, because a labelled group makes repeating
        // the version in every row pure noise.
        ry += VERSION_SEP_GAP;
        let header = format!(
            "v{} — {}",
            group.version,
            format_release_date(&group.entries[0].created_at)
        );
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(&header, x, ry);
        let rule_x = x + ctx.measure_text(&header) + 8.0;
        ctx.set_stroke_color(&tt.separator);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(rule_x, ry + VERSION_SEP_LABEL_H / 2.0);
        ctx.line_to(x + w, ry + VERSION_SEP_LABEL_H / 2.0);
        ctx.stroke();
        ry += VERSION_SEP_LABEL_H + VERSION_SEP_GAP;
        for entry in &group.entries {
            let label = release_row_label(
                ctx,
                platform_label(&entry.platform),
                &format_file_size_mb(entry.file_size),
                label_max_w,
            );
            draw_link_row(
                ctx,
                coord,
                layer_id,
                &format!("cloud_overlay:download:release:{}:{}", entry.platform, entry.version),
                WidgetRect::new(x, ry, w, ROW_H - ROW_GAP),
                &label,
                Icon::CloudDownload,
                tt,
            );
            ry += ROW_H;
        }
    }
}

/// Human label for a platform id (`mlc_updater::platform`'s own ids) —
/// mirrors the landing page's own `detectPlatform()` labels
/// (`mylittlechart-landing/web/index.html`). Architecture is stated
/// explicitly rather than in vendor marketing names: "x86-64"/"arm64" is
/// what the artifact actually is, it sorts honestly, and it does not break
/// the day a Windows/arm64 build exists (owner, 2026-09-05). Unrecognised
/// ids (a future platform this build doesn't know about yet) fall back to
/// the id itself rather than hiding the row.
fn platform_label(id: &str) -> &str {
    match id {
        "win64" => "Windows (x86-64)",
        "macos-arm64" => "macOS (arm64)",
        "macos-x64" => "macOS (x86-64)",
        "linux64" => "Linux (x86-64)",
        other => other,
    }
}

/// Alphabetical order key for a platform row: lowercased label, because a
/// naive byte sort would put "Windows" (W) before "macOS" (m). Catalogue
/// order is Linux → macOS → Windows (owner, 2026-09-05).
fn platform_sort_key(id: &str) -> String {
    platform_label(id).to_lowercase()
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
pub(crate) fn draw_expand_row(
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
pub(crate) fn draw_link_row(
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

pub(crate) fn draw_status(
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

pub(crate) fn wrap_text(text: &str, width: usize) -> Vec<String> {
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
    use super::{group_release_rows, platform_label, release_rows, wrap_text};
    use crate::ui::modal_settings::CloudReleaseEntry;

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

    fn entry(platform: &str, version: &str) -> CloudReleaseEntry {
        CloudReleaseEntry {
            platform: platform.to_string(),
            version: version.to_string(),
            file_size: 1,
            download_url: String::new(),
            created_at: "2026-09-04T00:00:00Z".to_string(),
        }
    }

    /// The catalogue lists the complete wave — the visitor's own platform
    /// included. It used to drop that row ("the primary action covers it"),
    /// and the owner read the 0.5.588 group of three as a missing binary
    /// (2026-09-04).
    #[test]
    fn release_rows_keep_the_visitors_own_platform() {
        let releases = vec![
            entry("linux64", "0.5.588"),
            entry("macos-x64", "0.5.588"),
            entry("macos-arm64", "0.5.588"),
            entry("win64", "0.5.588"),
            entry("win64", "0.5.587"),
        ];
        let rows = release_rows(&releases);
        assert_eq!(rows.len(), 5, "own platform's current build stays in the catalogue");
        // Current builds first (newest-first server order preserved), then
        // older ones.
        assert_eq!(rows[4].version, "0.5.587");
    }

    /// Adjacent rows sharing a version fold into one block; the same version
    /// on both sides of the current/older boundary stays two blocks.
    #[test]
    fn group_release_rows_folds_adjacent_only() {
        let releases = vec![
            entry("win64", "0.5.588"),
            entry("linux64", "0.5.588"),
            entry("win64", "0.5.587"),
        ];
        let groups = group_release_rows(&release_rows(&releases));
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].version, "0.5.588");
        assert_eq!(groups[0].entries.len(), 2);
        assert_eq!(groups[1].version, "0.5.587");
    }

    /// Inside a version block, platforms read alphabetically by label:
    /// Linux → macOS (arm64) → macOS (x86-64) → Windows — not the publish
    /// order the server happened to answer in (owner, 2026-09-05). The sort
    /// key is lowercased, or ASCII would park "Windows" ahead of "macOS".
    #[test]
    fn group_entries_sort_alphabetically_by_label() {
        let releases = vec![
            entry("win64", "0.5.588"),
            entry("macos-x64", "0.5.588"),
            entry("linux64", "0.5.588"),
            entry("macos-arm64", "0.5.588"),
        ];
        let groups = group_release_rows(&release_rows(&releases));
        assert_eq!(groups.len(), 1);
        let labels: Vec<&str> = groups[0].entries.iter().map(|e| platform_label(&e.platform)).collect();
        assert_eq!(
            labels,
            vec!["Linux (x86-64)", "macOS (arm64)", "macOS (x86-64)", "Windows (x86-64)"]
        );
    }
}
