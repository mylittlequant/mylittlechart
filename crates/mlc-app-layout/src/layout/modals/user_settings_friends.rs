//! The settings modal's **Friends** tab — renderer.
//!
//! State lives in `UserSettingsState` (`friend_*`), wording in
//! `mlc_core::i18n::FriendsKey`, the status line and invite rows in
//! `crate::ui::friends`. Accounts are hidden from search by default, so the tab
//! is built around the three ways in: a personal invite link or code
//! (generate, copy, redeem), a request to a public account (the requester is a
//! subscriber until it approves), and the lists those produce. A nick search
//! that finds nothing reads the same for a missing and a hidden account.
//! `ToolbarTheme` tokens only.

use mlc_core::engine::render::{draw_svg_icon, RenderContext};
use mlc_core::i18n::{current_language, FriendsKey};
use uzor::input::Sense;
use uzor::render::{TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;
use uzor::WidgetId;

use crate::layout::render_chart::FrameTheme;
use crate::layout::render_frame::UserSettingsResult;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::ui::friends::hit;
use crate::ui::modal_settings::UserSettingsState;
use crate::ui::scroll_widget::ScrollableContainer;
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::types::WidgetState;
use crate::ui::widgets::{draw_input, draw_input_cursor, fit_text_to_width, InputConfig, WidgetTheme};
use crate::ui::Icon;

/// Text-field store id of the nick search box. A literal for the same reason
/// the checkout's is: `mlc-app`, where the field is registered, is downstream.
const NICK_FIELD_ID: &str = "friends_nick";
/// Text-field store id of the invite redeem box.
const CODE_FIELD_ID: &str = "friends_invite_code";

const ROW_H: f64 = 30.0;
const FIELD_H: f64 = 32.0;
const FONT: &str = "12px sans-serif";

/// Draw the Friends tab into `viewport`.
pub(super) fn render_friends_tab(
    ctx: &mut dyn RenderContext,
    viewport: WidgetRect,
    state: &UserSettingsState,
    tt: &ToolbarTheme,
    frame_theme: &FrameTheme,
    scroll_widget_theme: &WidgetTheme,
    current_time_ms: u64,
    coord: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
) {
    let lang = current_language();
    let container = ScrollableContainer::new(viewport, &state.account_tab_scroll, None);
    container.begin(ctx);
    let x = viewport.x;
    let w = container.content_width();
    let top = container.content_y();
    let mut y = top;

    // ── Nick search ──────────────────────────────────────────────────────
    let glass = WidgetRect::new(x, y, FIELD_H, FIELD_H);
    ctx.set_fill_color(&tt.dropdown_bg);
    ctx.fill_rounded_rect(glass.x, glass.y, glass.width, glass.height, 6.0);
    draw_svg_icon(ctx, Icon::Search.svg(), glass.x + 8.0, glass.y + 8.0, 16.0, 16.0, &tt.item_text);
    coord.register_on_layer(format!("user_settings:{}", hit::NICK).as_str(), glass, Sense::CLICK, layer_id);
    let nick_rect = WidgetRect::new(x + FIELD_H + 4.0, y, (w - FIELD_H - 4.0).max(40.0), FIELD_H);
    let query = text_box(
        ctx,
        coord,
        layer_id,
        tt,
        frame_theme,
        current_time_ms,
        NICK_FIELD_ID,
        hit::NICK,
        nick_rect,
        FriendsKey::NickPlaceholder.get(lang),
        result,
    );
    y += FIELD_H + 8.0;

    if state.friend_hits.is_empty() {
        if query.trim().chars().count() >= 2 {
            // One sentence for "no such nick" and "hidden account": nothing
            // on the client can tell them apart.
            muted_line(ctx, tt, x, y, FriendsKey::NoSuchUser.get(lang));
            y += 22.0;
        }
    } else {
        for nick in state.friend_hits.iter().take(6) {
            y = person_row(
                ctx,
                coord,
                layer_id,
                tt,
                x,
                y,
                w,
                &format!("@{nick}"),
                nick,
                &[(FriendsKey::BtnRequest.get(lang), "add"), (FriendsKey::BtnSubscribe.get(lang), "sub")],
            );
        }
    }
    if let Some(note) = &state.friend_note {
        let color = if note.is_error() { &tt.danger } else { &tt.item_text_muted };
        ctx.set_font(FONT);
        ctx.set_fill_color(color);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        let line = fit_text_to_width(ctx, &note.text(lang), w);
        ctx.fill_text(&line, x, y + 8.0);
        y += 22.0;
    }

    // ── Redeem a link or code ────────────────────────────────────────────
    y += 4.0;
    let accept = FriendsKey::BtnAcceptInvite.get(lang);
    let accept_w = button_w(ctx, accept);
    let code_rect = WidgetRect::new(x, y, (w - accept_w - 8.0).max(40.0), FIELD_H);
    text_box(
        ctx,
        coord,
        layer_id,
        tt,
        frame_theme,
        current_time_ms,
        CODE_FIELD_ID,
        hit::INVITE_CODE,
        code_rect,
        FriendsKey::InviteCodePlaceholder.get(lang),
        result,
    );
    button(
        ctx,
        coord,
        layer_id,
        tt,
        WidgetRect::new(x + w - accept_w, y + 4.0, accept_w, 24.0),
        accept,
        &format!("user_settings:{}", hit::INVITE_ACCEPT),
    );
    y += FIELD_H + 12.0;

    // ── My invite links ──────────────────────────────────────────────────
    y = heading(ctx, tt, x, y, FriendsKey::SecInvites.get(lang));
    let create = FriendsKey::BtnCreateInvite.get(lang);
    let create_w = button_w(ctx, create).min(w);
    button(
        ctx,
        coord,
        layer_id,
        tt,
        WidgetRect::new(x, y, create_w, 26.0),
        create,
        &format!("user_settings:{}", hit::INVITE_CREATE),
    );
    y += 34.0;
    if state.friend_invites.is_empty() {
        muted_line(ctx, tt, x, y, FriendsKey::EmptyInvites.get(lang));
        y += 22.0;
    } else {
        for row in state.friend_invites.iter().take(10) {
            y = titled_row(
                ctx,
                coord,
                layer_id,
                tt,
                x,
                y,
                w,
                &format!("{}  {}", row.label(), row.meta(lang)),
                &row.token,
                &[(FriendsKey::BtnCopy.get(lang), "icopy"), (FriendsKey::BtnRevoke.get(lang), "irevoke")],
            );
        }
    }

    // ── Requests ─────────────────────────────────────────────────────────
    y = heading(ctx, tt, x, y + 6.0, FriendsKey::SecIncoming.get(lang));
    if state.friend_incoming.is_empty() {
        muted_line(ctx, tt, x, y, FriendsKey::EmptyRequests.get(lang));
        y += 22.0;
    } else {
        for nick in state.friend_incoming.iter().take(8) {
            y = person_row(
                ctx,
                coord,
                layer_id,
                tt,
                x,
                y,
                w,
                &format!("@{nick}"),
                nick,
                &[(FriendsKey::BtnApprove.get(lang), "approve"), (FriendsKey::BtnDecline.get(lang), "decline")],
            );
        }
    }
    y = heading(ctx, tt, x, y + 6.0, FriendsKey::SecOutgoing.get(lang));
    if state.friend_outgoing.is_empty() {
        muted_line(ctx, tt, x, y, FriendsKey::EmptyRequests.get(lang));
        y += 22.0;
    } else {
        for nick in state.friend_outgoing.iter().take(8) {
            muted_line(ctx, tt, x, y, &format!("@{nick}  {}", FriendsKey::Pending.get(lang)));
            y += 22.0;
        }
    }

    // ── Lists ────────────────────────────────────────────────────────────
    y = list_section(
        ctx,
        coord,
        layer_id,
        tt,
        (x, y, w),
        FriendsKey::SecFriends.get(lang),
        FriendsKey::EmptyFriends.get(lang),
        &state.friend_nicks,
        12,
        &[(FriendsKey::BtnRemove.get(lang), "remove"), (FriendsKey::BtnBlock.get(lang), "block")],
    );
    y = list_section(
        ctx,
        coord,
        layer_id,
        tt,
        (x, y, w),
        FriendsKey::SecSubscribers.get(lang),
        FriendsKey::EmptySubscribers.get(lang),
        &state.friend_subscribers,
        12,
        &[],
    );
    y = list_section(
        ctx,
        coord,
        layer_id,
        tt,
        (x, y, w),
        FriendsKey::SecSubscriptions.get(lang),
        FriendsKey::EmptySubscriptions.get(lang),
        &state.friend_subscriptions,
        8,
        &[(FriendsKey::BtnUnsubscribe.get(lang), "unsub")],
    );
    y = list_section(
        ctx,
        coord,
        layer_id,
        tt,
        (x, y, w),
        FriendsKey::SecBlocked.get(lang),
        FriendsKey::EmptyBlocked.get(lang),
        &state.friend_blocked,
        8,
        &[(FriendsKey::BtnUnblock.get(lang), "unblock")],
    );

    let scroll_result = container.end(ctx, y + 8.0 - top, scroll_widget_theme);
    result.scroll_viewport_rect = Some(viewport);
    result.scroll_content_height = scroll_result.content_height;
    result.scrollbar_handle_rect = scroll_result.handle_rect;
    result.scrollbar_track_rect = scroll_result.track_rect;
    result.scroll_viewport_height = scroll_result.viewport_height;
    if let Some(ref hr) = result.scrollbar_handle_rect {
        let inflated = uzor::types::Rect::new(hr.x - 5.0, hr.y, hr.width + 10.0, hr.height);
        coord.register_on_layer("user_settings:scrollbar_handle", inflated, Sense::DRAG, layer_id);
    }
    if let Some(ref tr) = result.scrollbar_track_rect {
        coord.register_on_layer(
            "user_settings:scrollbar_track",
            uzor::types::Rect::new(tr.x, tr.y, tr.width, tr.height),
            Sense::CLICK,
            layer_id,
        );
    }
    if let Some(ref vp) = result.scroll_viewport_rect {
        coord.register_on_layer(
            "user_settings:scroll_viewport",
            uzor::types::Rect::new(vp.x, vp.y, vp.width, vp.height),
            Sense::SCROLL,
            layer_id,
        );
    }
}

/// A titled list with one row per nick and the same actions on each.
fn list_section(
    ctx: &mut dyn RenderContext,
    coord: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    tt: &ToolbarTheme,
    (x, y, w): (f64, f64, f64),
    title: &str,
    empty: &str,
    nicks: &[String],
    cap: usize,
    actions: &[(&str, &str)],
) -> f64 {
    let mut y = heading(ctx, tt, x, y + 6.0, title);
    if nicks.is_empty() {
        muted_line(ctx, tt, x, y, empty);
        return y + 22.0;
    }
    for nick in nicks.iter().take(cap) {
        y = person_row(ctx, coord, layer_id, tt, x, y, w, &format!("@{nick}"), nick, actions);
    }
    y
}

fn heading(ctx: &mut dyn RenderContext, tt: &ToolbarTheme, x: f64, y: f64, text: &str) -> f64 {
    ctx.set_font("600 12px sans-serif");
    ctx.set_fill_color(&tt.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(text, x, y + 8.0);
    y + 24.0
}

fn muted_line(ctx: &mut dyn RenderContext, tt: &ToolbarTheme, x: f64, y: f64, text: &str) {
    ctx.set_font(FONT);
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(text, x, y + 8.0);
}

/// Width of a small button for `label`.
fn button_w(ctx: &mut dyn RenderContext, label: &str) -> f64 {
    ctx.set_font(FONT);
    ctx.measure_text(label) + 20.0
}

fn button(
    ctx: &mut dyn RenderContext,
    coord: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    tt: &ToolbarTheme,
    r: WidgetRect,
    label: &str,
    hit_id: &str,
) {
    let hovered = coord.is_hovered(&WidgetId::from(hit_id));
    ctx.set_fill_color(if hovered { &tt.item_bg_hover } else { &tt.dropdown_bg });
    ctx.fill_rounded_rect(r.x, r.y, r.width, r.height, 4.0);
    ctx.set_stroke_color(&tt.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(r.x, r.y, r.width, r.height, 4.0);
    ctx.set_font(FONT);
    ctx.set_fill_color(&tt.item_text);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    let shown = fit_text_to_width(ctx, label, (r.width - 8.0).max(1.0));
    ctx.fill_text(&shown, r.center_x(), r.center_y());
    ctx.set_text_align(TextAlign::Left);
    coord.register_on_layer(hit_id, r, Sense::CLICK | Sense::HOVER, layer_id);
}

/// One text input drawn like the other settings boxes, its geometry published
/// for the store. Returns the text it holds.
fn text_box(
    ctx: &mut dyn RenderContext,
    coord: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    tt: &ToolbarTheme,
    frame_theme: &FrameTheme,
    time_ms: u64,
    store_id: &str,
    content_key: &str,
    rect: WidgetRect,
    placeholder: &str,
    result: &mut UserSettingsResult,
) -> String {
    let wid = WidgetId::from(store_id);
    let text = coord.text_fields().text(&wid).to_string();
    let cursor = coord.text_fields().cursor(&wid);
    let (sel_start, sel_end) = match coord.text_fields().selection_range(&wid) {
        Some((lo, hi)) => (Some(lo), Some(hi)),
        None => (None, None),
    };
    let engaged = coord.text_fields().is_engaged(&wid);
    let widget_theme = toolbar_to_widget_theme(tt, frame_theme);
    let config = InputConfig::new(&text)
        .with_focused(engaged)
        .with_cursor(cursor)
        .with_placeholder(placeholder)
        .with_selection(sel_start, sel_end);
    let drawn = draw_input(ctx, &config, WidgetState::Normal, rect, &widget_theme);
    coord.register_on_layer(
        format!("user_settings:{content_key}").as_str(),
        rect,
        Sense::CLICK | Sense::DRAG,
        layer_id,
    );
    result.content_items.push((content_key.to_string(), rect));
    result.input_char_positions.push((content_key.to_string(), drawn.char_x_positions));
    if engaged && coord.text_fields().cursor_visible(time_ms) {
        draw_input_cursor(ctx, drawn.cursor_x, drawn.cursor_y, drawn.cursor_height, &tt.item_text);
    }
    ctx.set_text_align(TextAlign::Left);
    text
}

/// A row with a title on the left and buttons on the right; each button's hit
/// id is `user_settings:friend_{action}:{key}`. Returns the y after the row.
fn titled_row(
    ctx: &mut dyn RenderContext,
    coord: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    tt: &ToolbarTheme,
    x: f64,
    y: f64,
    w: f64,
    title: &str,
    key: &str,
    actions: &[(&str, &str)],
) -> f64 {
    let mut widths: Vec<(f64, &str, &str)> = Vec::new();
    let mut used = 0.0;
    for (label, action) in actions.iter().rev() {
        let bw = button_w(ctx, label);
        used += bw + 6.0;
        widths.push((bw, label, action));
    }
    ctx.set_font(FONT);
    ctx.set_fill_color(&tt.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    let shown = fit_text_to_width(ctx, title, (w - used - 8.0).max(24.0));
    ctx.fill_text(&shown, x, y + 13.0);
    let mut bx = x + w;
    for (bw, label, action) in widths {
        bx -= bw + 6.0;
        button(
            ctx,
            coord,
            layer_id,
            tt,
            WidgetRect::new(bx, y + 1.0, bw, 24.0),
            label,
            &format!("user_settings:friend_{action}:{key}"),
        );
    }
    y + ROW_H
}

/// [`titled_row`] for a nick: the title is `@nick`, the key the nick itself.
fn person_row(
    ctx: &mut dyn RenderContext,
    coord: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    tt: &ToolbarTheme,
    x: f64,
    y: f64,
    w: f64,
    title: &str,
    nick: &str,
    actions: &[(&str, &str)],
) -> f64 {
    titled_row(ctx, coord, layer_id, tt, x, y, w, title, nick, actions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlc_core::i18n::Language;
    use crate::layout::modals::user_settings::render_user_settings_modal;
    use crate::ui::friends::{FriendInviteRow, FriendNote};
    use crate::ui::modal_settings::{CredentialsState, UserSettingsTab};

    #[derive(Default)]
    struct Rec {
        texts: Vec<String>,
    }

    impl uzor::render::Painter for Rec {
        fn save(&mut self) {}
        fn restore(&mut self) {}
        fn translate(&mut self, _x: f64, _y: f64) {}
        fn rotate(&mut self, _angle: f64) {}
        fn scale(&mut self, _x: f64, _y: f64) {}
        fn set_fill_color(&mut self, _color: &str) {}
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
            text.chars().count() as f64 * 6.0
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
        fn fill_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
        fn stroke_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
    }
    impl uzor::render::GradientPainter for Rec {}
    impl uzor::render::UiEffectHelpers for Rec {}
    impl uzor::render::BatchPainter for Rec {}
    impl uzor::render::RenderContext for Rec {
        fn dpr(&self) -> f64 {
            1.0
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

    fn tab_state(tab: UserSettingsTab) -> UserSettingsState {
        let mut st = UserSettingsState::default();
        st.is_open = true;
        st.account_signed_in = true;
        st.active_tab = tab;
        st
    }

    fn draw(
        st: &UserSettingsState,
        coord: &mut uzor::input::InputCoordinator,
    ) -> (Vec<String>, UserSettingsResult) {
        let mut ctx = Rec::default();
        let result = render_user_settings_modal(
            &mut ctx,
            1280.0,
            1000.0,
            st,
            &CredentialsState::default(),
            &FrameTheme::default(),
            &ToolbarTheme::default(),
            0,
            coord,
            false,
        );
        (ctx.texts, result)
    }

    fn has(texts: &[String], needle: &str) -> bool {
        texts.iter().any(|t| t == needle)
    }

    fn registered(coord: &uzor::input::InputCoordinator, id: &str) -> bool {
        coord.widget_rect(&WidgetId::from(id)).is_some()
    }

    #[test]
    fn a_new_account_is_hidden_and_the_account_tab_says_so() {
        let st = tab_state(UserSettingsTab::Account);
        assert!(!st.discoverable, "hidden until the person turns it on");
        let mut coord = uzor::input::InputCoordinator::new();
        let (texts, _) = draw(&st, &mut coord);
        assert!(has(&texts, "Show me in search"), "{texts:?}");
        assert!(has(&texts, "Off"), "{texts:?}");
        assert!(texts.iter().any(|t| t.contains("Off by default")), "{texts:?}");
        assert!(registered(&coord, "user_settings:discoverable"), "the toggle takes clicks");
    }

    #[test]
    fn the_toggle_reads_on_when_the_account_is_findable() {
        let mut st = tab_state(UserSettingsTab::Account);
        st.discoverable = true;
        let mut coord = uzor::input::InputCoordinator::new();
        let (texts, _) = draw(&st, &mut coord);
        assert!(has(&texts, "On") && !has(&texts, "Off"), "{texts:?}");
    }

    #[test]
    fn the_friends_tab_draws_every_section_with_its_empty_state() {
        let st = tab_state(UserSettingsTab::Friends);
        let mut coord = uzor::input::InputCoordinator::new();
        let (texts, result) = draw(&st, &mut coord);
        for s in [
            "Invite links",
            "New invite link",
            "No invite links.",
            "Incoming requests",
            "Sent requests",
            "Friends",
            "Subscribers",
            "Subscriptions",
            "Blocked",
            "Accept",
            "No subscribers.",
            "No one blocked.",
        ] {
            assert!(has(&texts, s), "{s}: {texts:?}");
        }
        assert!(registered(&coord, "user_settings:friend_invite"), "create a link");
        assert!(registered(&coord, "user_settings:friend_accept"), "redeem a link or code");
        for key in ["friend_nick", "friend_code"] {
            assert!(result.content_items.iter().any(|(k, _)| k == key), "{key} publishes its geometry");
        }
    }

    #[test]
    fn incoming_requests_offer_approve_and_decline_and_a_sent_one_reads_pending() {
        let mut st = tab_state(UserSettingsTab::Friends);
        st.friend_incoming = vec!["bob".into()];
        st.friend_outgoing = vec!["eve".into()];
        let mut coord = uzor::input::InputCoordinator::new();
        let (texts, _) = draw(&st, &mut coord);
        assert!(has(&texts, "@bob") && has(&texts, "Approve") && has(&texts, "Decline"), "{texts:?}");
        assert!(registered(&coord, "user_settings:friend_approve:bob"));
        assert!(registered(&coord, "user_settings:friend_decline:bob"));
        assert!(has(&texts, "@eve  pending"), "{texts:?}");
        assert!(!registered(&coord, "user_settings:friend_approve:eve"), "a sent request has no approve");
    }

    #[test]
    fn friends_subscribers_subscriptions_and_blocked_carry_their_own_actions() {
        let mut st = tab_state(UserSettingsTab::Friends);
        st.friend_nicks = vec!["ann".into()];
        st.friend_subscribers = vec!["sub1".into()];
        st.friend_subscriptions = vec!["sub2".into()];
        st.friend_blocked = vec!["bad".into()];
        let mut coord = uzor::input::InputCoordinator::new();
        let (texts, _) = draw(&st, &mut coord);
        for nick in ["@ann", "@sub1", "@sub2", "@bad"] {
            assert!(has(&texts, nick), "{nick}: {texts:?}");
        }
        assert!(registered(&coord, "user_settings:friend_remove:ann"));
        assert!(registered(&coord, "user_settings:friend_block:ann"));
        assert!(registered(&coord, "user_settings:friend_unsub:sub2"));
        assert!(registered(&coord, "user_settings:friend_unblock:bad"));
        assert!(!registered(&coord, "user_settings:friend_remove:sub1"), "a subscriber is listed, not managed here");
    }

    #[test]
    fn invite_links_list_with_copy_and_revoke_and_never_print_the_whole_token() {
        let mut st = tab_state(UserSettingsTab::Friends);
        st.friend_invites = vec![FriendInviteRow {
            token: "abcdefgh12345678".into(),
            uses: 0,
            max_uses: 1,
            expires_at: "2026-10-15T00:00:00+00:00".into(),
        }];
        let mut coord = uzor::input::InputCoordinator::new();
        let (texts, _) = draw(&st, &mut coord);
        assert!(texts.iter().any(|t| t.contains("0/1 used, expires 2026-10-15")), "{texts:?}");
        assert!(!texts.iter().any(|t| t.contains("12345678")), "the token stays off the row: {texts:?}");
        assert!(registered(&coord, "user_settings:friend_icopy:abcdefgh12345678"));
        assert!(registered(&coord, "user_settings:friend_irevoke:abcdefgh12345678"));
        assert!(!has(&texts, "No invite links."));
    }

    #[test]
    fn a_search_with_no_hits_says_no_such_user_whatever_the_reason() {
        use uzor::input::TextFieldConfig;
        let st = tab_state(UserSettingsTab::Friends);
        let wid = WidgetId::from(NICK_FIELD_ID);
        let mut coord = uzor::input::InputCoordinator::new();
        coord.text_fields_mut().register(wid.clone(), TextFieldConfig::text().with_max_len(64));
        coord.text_fields_mut().set_text(&wid, "ghostuser");
        let (texts, _) = draw(&st, &mut coord);
        assert!(has(&texts, "No such user."), "{texts:?}");
        assert!(!texts.iter().any(|t| t.to_lowercase().contains("hidden") || t.to_lowercase().contains("private")));
    }

    #[test]
    fn a_visible_account_in_the_hits_takes_a_request_or_a_subscription() {
        let mut st = tab_state(UserSettingsTab::Friends);
        st.friend_hits = vec!["carol".into()];
        let mut coord = uzor::input::InputCoordinator::new();
        let (texts, _) = draw(&st, &mut coord);
        assert!(has(&texts, "@carol") && has(&texts, "Request") && has(&texts, "Subscribe"), "{texts:?}");
        assert!(registered(&coord, "user_settings:friend_add:carol"));
        assert!(registered(&coord, "user_settings:friend_sub:carol"));
    }

    #[test]
    fn the_status_line_is_worded_from_the_note_in_the_active_language() {
        let mut st = tab_state(UserSettingsTab::Friends);
        st.friend_note = Some(FriendNote::Accepted { nick: Some("nix".into()) });
        let mut coord = uzor::input::InputCoordinator::new();
        let (texts, _) = draw(&st, &mut coord);
        assert!(has(&texts, "Invite accepted. You are now friends with nix."), "{texts:?}");
        st.friend_note = Some(FriendNote::SignInToAccept);
        let (texts, _) = draw(&st, &mut coord);
        assert!(has(&texts, "Sign in to accept the friend invite."), "{texts:?}");
    }

    #[test]
    fn every_label_of_the_tab_exists_in_every_language() {
        for lang in Language::all() {
            for key in [
                FriendsKey::ShowInSearch,
                FriendsKey::BtnApprove,
                FriendsKey::BtnDecline,
                FriendsKey::SecSubscribers,
                FriendsKey::NoSuchUser,
            ] {
                assert!(!key.get(*lang).is_empty(), "{key:?} in {lang:?}");
            }
        }
    }
}
