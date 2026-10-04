//! Pencil menu, create/join dialogs, the folder editor, and the
//! "send chart to a chat" room picker. The Messages body in
//! [`super::messenger_overlay`] decides when each one is on screen.
//! Field ids are the same string literals `mlc-app` registers
//! (`dm_group_title`, …). This crate does not depend on `mlc-app`.

use mlc_core::engine::render::{RenderContext, TextAlign, TextBaseline};
use mlc_messenger_model::{Capabilities, Folder, MessengerProvider};
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::types::Rect as WidgetRect;
use uzor::WidgetId;

use crate::layout::render_chart::FrameTheme;
use crate::ui::modal_settings::{CommunityState, MessengerDialog};
use crate::ui::scroll_widget::ScrollableContainer;
use crate::ui::toolbar_theme::ToolbarTheme;

use super::cloud_overlay::{
    draw_action_row, register_scrollbar_hit_zones, scroll_config, CloudOverlayResult, FIELD_H, ROW_H,
};
use super::community::CommunityListPaneResult;
use super::dm_overlay::draw_field_shaped;

const FIELD_GAP: f64 = 8.0;
const MENU_W: f64 = 188.0;
const MENU_ROW_H: f64 = 32.0;
const PANEL_W: f64 = 320.0;
const HIT_CAP: usize = 6;
const JOIN_CAP: usize = 12;
const ROOM_CAP: usize = 40;

const F_GROUP_TITLE: &str = "dm_group_title";
const F_GROUP_MEMBER: &str = "dm_group_member";
const F_CHANNEL_TITLE: &str = "dm_channel_title";
const F_CHANNEL_TOPIC: &str = "dm_channel_topic";
const F_JOIN_QUERY: &str = "dm_join_query";
const F_FOLDER_NAME: &str = "dm_folder_name";

const ICON_LABELS: [&str; 6] = ["Candles", "Folder", "Group", "Channel", "Bookmark", "Person"];

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_folder_editor(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    viewport_h: f64,
    state: &CommunityState,
    provider: &dyn MessengerProvider,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    result: &mut CloudOverlayResult,
    list_result: &mut CommunityListPaneResult,
) {
    let Some(editor) = state.messenger_folder_editor.as_ref() else { return };
    let viewport = WidgetRect::new(x, y, w, viewport_h);
    coord.register_on_layer("cloud_overlay:scroll_viewport", viewport, Sense::SCROLL, layer_id);
    let scrollable = ScrollableContainer::new(viewport, &state.list_scroll, Some(scroll_config()));
    scrollable.begin(ctx);
    let origin = scrollable.content_y();
    let mut ly = origin + 8.0;
    let lx = x + 12.0;
    let lw = (scrollable.content_width() - 24.0).max(40.0);

    section_label(ctx, lx, ly, "Folders", tt);
    ly += 22.0;

    let folders = provider.folders();
    for folder in folders.iter().filter(|f| f.id.is_none()) {
        plain_line(ctx, lx, ly, lw, &folder.name, &tt.item_text_muted);
        ly += 22.0;
    }

    section_label(ctx, lx, ly + 4.0, "Your folders", tt);
    ly += 26.0;
    let mut custom: Vec<&Folder> = folders.iter().filter(|f| f.id.is_some()).collect();
    custom.sort_by(|a, b| a.order.partial_cmp(&b.order).unwrap_or(std::cmp::Ordering::Equal));
    if custom.is_empty() {
        plain_line(ctx, lx, ly, lw, "No custom folders.", &tt.item_text_muted);
        ly += 22.0;
    }
    for folder in &custom {
        let Some(tag) = folder.id.as_ref() else { continue };
        ly += draw_custom_folder_row(ctx, coord, layer_id, lx, ly, lw, folder, tag.as_str(), tt);
    }

    ly += 8.0;
    section_label(ctx, lx, ly, if editor.tag.is_some() { "Edit folder" } else { "New folder" }, tt);
    ly += 24.0;

    let name_rect = WidgetRect::new(lx, ly, lw, FIELD_H);
    let (rect, chars) = draw_field_shaped(
        ctx, coord, layer_id, "cloud_overlay:dm:folder_name_input", F_FOLDER_NAME, name_rect,
        "Folder name", ft, tt, current_time_ms, 6.0, 8.0,
    );
    result.dm_folder_name_rect = Some(rect);
    result.dm_folder_name_chars = chars;
    ly += FIELD_H + FIELD_GAP;

    ly += draw_icon_row(ctx, coord, layer_id, lx, ly, lw, editor.icon_idx, tt);
    ly += 4.0;
    section_label(ctx, lx, ly, "Include", tt);
    ly += 20.0;
    ly += draw_toggle(ctx, coord, layer_id, lx, ly, lw, "cloud_overlay:dm:folder_edit_inc:dms", "All direct messages", editor.all_dms, tt);
    ly += draw_toggle(ctx, coord, layer_id, lx, ly, lw, "cloud_overlay:dm:folder_edit_inc:groups", "All groups", editor.all_groups, tt);
    ly += draw_toggle(ctx, coord, layer_id, lx, ly, lw, "cloud_overlay:dm:folder_edit_inc:channels", "All channels", editor.all_channels, tt);

    section_label(ctx, lx, ly + 4.0, "Rooms", tt);
    ly += 24.0;
    let rooms = provider.rooms();
    let shown = rooms.iter().take(ROOM_CAP);
    let mut any_room = false;
    for (index, room) in shown.enumerate() {
        any_room = true;
        let on = editor.rooms.iter().any(|id| id == room.id.as_str());
        let wid = format!("cloud_overlay:dm:folder_edit_room:{index}");
        ly += draw_toggle(ctx, coord, layer_id, lx, ly, lw, &wid, &room.name, on, tt);
    }
    if !any_room {
        plain_line(ctx, lx, ly, lw, "No chats yet.", &tt.item_text_muted);
        ly += 22.0;
    }

    section_label(ctx, lx, ly + 4.0, "Exclude", tt);
    ly += 24.0;
    ly += draw_toggle(ctx, coord, layer_id, lx, ly, lw, "cloud_overlay:dm:folder_edit_exc:muted", "Muted", editor.exclude_muted, tt);
    ly += draw_toggle(ctx, coord, layer_id, lx, ly, lw, "cloud_overlay:dm:folder_edit_exc:read", "Already read", editor.exclude_read, tt);
    ly += draw_toggle(ctx, coord, layer_id, lx, ly, lw, "cloud_overlay:dm:folder_edit_exc:archived", "Archived", editor.exclude_archived, tt);

    ly += 8.0;
    let btn_w = (lw - 8.0) / 2.0;
    draw_action_row(
        ctx, coord, layer_id, "cloud_overlay:dm:folder_edit_save",
        WidgetRect::new(lx, ly, btn_w, ROW_H - 6.0), "Save", tt,
    );
    draw_text_button(
        ctx, coord, layer_id, "cloud_overlay:dm:folder_edit_cancel",
        WidgetRect::new(lx + btn_w + 8.0, ly, btn_w, ROW_H - 6.0), "Cancel", &tt.item_text, tt,
    );
    ly += ROW_H;
    if editor.tag.is_some() {
        draw_text_button(
            ctx, coord, layer_id, "cloud_overlay:dm:folder_edit_delete",
            WidgetRect::new(lx, ly, lw, ROW_H - 6.0), "Delete folder", &tt.danger, tt,
        );
        ly += ROW_H;
    }

    let content_h = (ly - origin).max(1.0);
    let widget_theme = crate::ui::widgets::types::WidgetTheme::default();
    let scroll_result = scrollable.end(ctx, content_h, &widget_theme);
    list_result.viewport_rect = Some(viewport);
    list_result.total_content_height = Some(content_h);
    list_result.viewport_height = Some(viewport_h.min(content_h));
    list_result.scrollbar_handle_rect = scroll_result.handle_rect;
    list_result.scrollbar_track_rect = scroll_result.track_rect;
    register_scrollbar_hit_zones(coord, layer_id, &scroll_result);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_popups(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    list_x: f64,
    list_w: f64,
    filter_row_h: f64,
    state: &CommunityState,
    provider: &dyn MessengerProvider,
    caps: Capabilities,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    result: &mut CloudOverlayResult,
) {
    if state.messenger_folder_editor.is_none() {
        if matches!(state.messenger_dialog, Some(MessengerDialog::Pencil)) {
            draw_pencil_menu(ctx, coord, layer_id, list_x, y + filter_row_h, list_w, caps, tt);
        }
    }
    match &state.messenger_dialog {
        Some(MessengerDialog::Group { members, members_can_add }) => {
            draw_group_dialog(
                ctx, coord, layer_id, x, y, w, h, members, *members_can_add, provider, ft, tt,
                current_time_ms, result,
            );
        }
        Some(MessengerDialog::Channel) => {
            draw_channel_dialog(ctx, coord, layer_id, x, y, w, h, ft, tt, current_time_ms, result);
        }
        Some(MessengerDialog::Join) => {
            draw_join_dialog(ctx, coord, layer_id, x, y, w, h, provider, ft, tt, current_time_ms, result);
        }
        _ => {}
    }
    if state.messenger_send_chart.is_some() {
        draw_send_chart_picker(ctx, coord, layer_id, x, y, w, h, provider, tt);
    }
}

fn draw_pencil_menu(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    list_x: f64,
    y: f64,
    list_w: f64,
    caps: Capabilities,
    tt: &ToolbarTheme,
) {
    let mut rows: Vec<(&str, &str)> = vec![("create_menu_message", "New message")];
    if caps.groups {
        rows.push(("create_menu_group", "New group"));
    }
    if caps.channels {
        rows.push(("create_menu_channel", "New channel"));
        rows.push(("create_menu_join", "Join channel"));
    }
    if caps.folders {
        rows.push(("create_menu_folder", "New folder"));
    }
    let menu_h = rows.len() as f64 * MENU_ROW_H + 8.0;
    let menu_x = list_x + list_w - MENU_W - 4.0;
    paint_panel(ctx, menu_x, y, MENU_W, menu_h, tt);
    let mut ry = y + 4.0;
    for (id, label) in rows {
        let rect = WidgetRect::new(menu_x + 4.0, ry, MENU_W - 8.0, MENU_ROW_H - 2.0);
        let wid = format!("cloud_overlay:dm:{id}");
        hover_fill(ctx, coord, &wid, rect, tt);
        label_left(ctx, &rect, label, &tt.item_text);
        coord.register_on_layer(wid.as_str(), rect, Sense::CLICK | Sense::HOVER, layer_id);
        ry += MENU_ROW_H;
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_group_dialog(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    members: &[String],
    members_can_add: bool,
    provider: &dyn MessengerProvider,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    result: &mut CloudOverlayResult,
) {
    let hits = provider.user_search();
    let hit_n = hits.len().min(HIT_CAP);
    let chip_h = if members.is_empty() { 0.0 } else { 28.0 };
    let panel_h = (36.0 + FIELD_H + FIELD_GAP + FIELD_H + FIELD_GAP + hit_n as f64 * 28.0 + chip_h + 28.0 + 36.0 + ROW_H + 16.0)
        .min(h - 24.0);
    let (px, py, pw) = centered(x, y, w, h, panel_h);
    paint_scrim(ctx, x, y, w, h);
    paint_panel(ctx, px, py, pw, panel_h, tt);
    let mut cy = py + 8.0;
    cy += dialog_title(ctx, coord, layer_id, px, cy, pw, "New group", tt);
    cy += dialog_field(
        ctx, coord, layer_id, px + 12.0, cy, pw - 24.0, "cloud_overlay:dm:group_title_input", F_GROUP_TITLE,
        "Group name", ft, tt, current_time_ms, &mut result.dm_group_title_rect, &mut result.dm_group_title_chars,
    );
    cy += dialog_field(
        ctx, coord, layer_id, px + 12.0, cy, pw - 24.0, "cloud_overlay:dm:group_member_input", F_GROUP_MEMBER,
        "Add a person", ft, tt, current_time_ms, &mut result.dm_group_member_rect, &mut result.dm_group_member_chars,
    );
    for (index, hit) in hits.iter().take(HIT_CAP).enumerate() {
        let rect = WidgetRect::new(px + 12.0, cy, pw - 24.0, 26.0);
        let wid = format!("cloud_overlay:dm:create_member:{index}");
        hover_fill(ctx, coord, &wid, rect, tt);
        let name = if hit.display_name.is_empty() { hit.user_id.as_str() } else { hit.display_name.as_str() };
        label_left(ctx, &rect, name, &tt.item_text);
        coord.register_on_layer(wid.as_str(), rect, Sense::CLICK | Sense::HOVER, layer_id);
        cy += 28.0;
    }
    if !members.is_empty() {
        draw_chips(ctx, coord, layer_id, px + 12.0, cy, pw - 24.0, members, &hits, tt);
        cy += chip_h;
    }
    cy += draw_toggle(
        ctx, coord, layer_id, px + 12.0, cy, pw - 24.0, "cloud_overlay:dm:create_members_can_add",
        "Members can add people", members_can_add, tt,
    );
    plain_line(ctx, px + 12.0, cy, pw - 24.0, "End-to-end encrypted. Only members can read this group.", &tt.item_text_muted);
    cy += 28.0;
    dialog_buttons(ctx, coord, layer_id, px + 12.0, cy, pw - 24.0, "Create", tt);
}

#[allow(clippy::too_many_arguments)]
fn draw_channel_dialog(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    result: &mut CloudOverlayResult,
) {
    let panel_h = 36.0 + FIELD_H + FIELD_GAP + FIELD_H + FIELD_GAP + 36.0 + ROW_H + 16.0;
    let (px, py, pw) = centered(x, y, w, h, panel_h);
    paint_scrim(ctx, x, y, w, h);
    paint_panel(ctx, px, py, pw, panel_h, tt);
    let mut cy = py + 8.0;
    cy += dialog_title(ctx, coord, layer_id, px, cy, pw, "New channel", tt);
    cy += dialog_field(
        ctx, coord, layer_id, px + 12.0, cy, pw - 24.0, "cloud_overlay:dm:channel_title_input", F_CHANNEL_TITLE,
        "Channel name", ft, tt, current_time_ms, &mut result.dm_channel_title_rect, &mut result.dm_channel_title_chars,
    );
    cy += dialog_field(
        ctx, coord, layer_id, px + 12.0, cy, pw - 24.0, "cloud_overlay:dm:channel_topic_input", F_CHANNEL_TOPIC,
        "Topic", ft, tt, current_time_ms, &mut result.dm_channel_topic_rect, &mut result.dm_channel_topic_chars,
    );
    plain_line(ctx, px + 12.0, cy, pw - 24.0, "Public channel. Posts are readable without a vault.", &tt.item_text_muted);
    cy += 28.0;
    dialog_buttons(ctx, coord, layer_id, px + 12.0, cy, pw - 24.0, "Create", tt);
}

#[allow(clippy::too_many_arguments)]
fn draw_join_dialog(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    provider: &dyn MessengerProvider,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    result: &mut CloudOverlayResult,
) {
    let rooms = provider.public_rooms();
    let n = rooms.len().min(JOIN_CAP);
    let panel_h = (36.0 + FIELD_H + FIELD_GAP + n as f64 * 32.0 + 36.0).min(h - 24.0).max(120.0);
    let (px, py, pw) = centered(x, y, w, h, panel_h);
    paint_scrim(ctx, x, y, w, h);
    paint_panel(ctx, px, py, pw, panel_h, tt);
    let mut cy = py + 8.0;
    cy += dialog_title(ctx, coord, layer_id, px, cy, pw, "Join a channel", tt);
    cy += dialog_field(
        ctx, coord, layer_id, px + 12.0, cy, pw - 24.0, "cloud_overlay:dm:join_query_input", F_JOIN_QUERY,
        "Search public channels", ft, tt, current_time_ms, &mut result.dm_join_query_rect, &mut result.dm_join_query_chars,
    );
    if rooms.is_empty() {
        plain_line(ctx, px + 12.0, cy, pw - 24.0, "No public channels.", &tt.item_text_muted);
        return;
    }
    for (index, room) in rooms.iter().take(JOIN_CAP).enumerate() {
        let rect = WidgetRect::new(px + 8.0, cy, pw - 16.0, 30.0);
        let label = if room.joined {
            format!("{} · Joined", room.name)
        } else {
            room.name.clone()
        };
        if room.joined {
            label_left(ctx, &rect, &label, &tt.item_text_muted);
        } else {
            let wid = format!("cloud_overlay:dm:join_room:{index}");
            hover_fill(ctx, coord, &wid, rect, tt);
            label_left(ctx, &rect, &label, &tt.item_text);
            coord.register_on_layer(wid.as_str(), rect, Sense::CLICK | Sense::HOVER, layer_id);
        }
        cy += 32.0;
    }
}

fn draw_send_chart_picker(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    provider: &dyn MessengerProvider,
    tt: &ToolbarTheme,
) {
    let rooms = provider.rooms();
    let n = rooms.len().min(JOIN_CAP);
    let panel_h = (36.0 + n as f64 * 36.0 + 12.0).min(h - 24.0).max(80.0);
    let (px, py, pw) = centered(x, y, w, h, panel_h);
    paint_scrim(ctx, x, y, w, h);
    paint_panel(ctx, px, py, pw, panel_h, tt);
    let mut cy = py + 8.0;
    cy += dialog_title(ctx, coord, layer_id, px, cy, pw, "Send to chat", tt);
    if rooms.is_empty() {
        plain_line(ctx, px + 12.0, cy, pw - 24.0, "No conversations yet.", &tt.item_text_muted);
        return;
    }
    for room in rooms.iter().take(JOIN_CAP) {
        let rect = WidgetRect::new(px + 8.0, cy, pw - 16.0, 34.0);
        let wid = format!("cloud_overlay:dm:send_chart_to:{}", room.id.as_str());
        hover_fill(ctx, coord, &wid, rect, tt);
        label_left(ctx, &rect, &room.name, &tt.item_text);
        coord.register_on_layer(wid.as_str(), rect, Sense::CLICK | Sense::HOVER, layer_id);
        cy += 36.0;
    }
}

fn draw_custom_folder_row(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    folder: &Folder,
    tag: &str,
    tt: &ToolbarTheme,
) -> f64 {
    let pick = WidgetRect::new(x, y, (w - 64.0).max(40.0), 26.0);
    let up = WidgetRect::new(pick.x + pick.width + 4.0, y, 28.0, 26.0);
    let down = WidgetRect::new(up.x + 32.0, y, 28.0, 26.0);
    let pick_id = format!("cloud_overlay:dm:folder_edit_pick:{tag}");
    let up_id = format!("cloud_overlay:dm:folder_edit_up:{tag}");
    let down_id = format!("cloud_overlay:dm:folder_edit_down:{tag}");
    hover_fill(ctx, coord, &pick_id, pick, tt);
    label_left(ctx, &pick, &folder.name, &tt.item_text);
    coord.register_on_layer(pick_id.as_str(), pick, Sense::CLICK | Sense::HOVER, layer_id);
    square_button(ctx, coord, layer_id, &up_id, up, "\u{2191}", tt);
    square_button(ctx, coord, layer_id, &down_id, down, "\u{2193}", tt);
    30.0
}

fn draw_icon_row(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    selected: u8,
    tt: &ToolbarTheme,
) -> f64 {
    let gap = 4.0;
    let bw = ((w - gap * 5.0) / 6.0).max(28.0);
    for (idx, label) in ICON_LABELS.iter().enumerate() {
        let rect = WidgetRect::new(x + idx as f64 * (bw + gap), y, bw, 26.0);
        let wid = format!("cloud_overlay:dm:folder_edit_icon:{idx}");
        let on = selected as usize == idx;
        if on {
            ctx.set_fill_color(&tt.accent);
            ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 4.0);
        } else {
            hover_fill(ctx, coord, &wid, rect, tt);
        }
        let color: &str = if on {
            crate::ui::widgets::avatar::contrasting_text_color(&tt.accent)
        } else {
            tt.item_text.as_str()
        };
        ctx.set_font("10px sans-serif");
        ctx.set_fill_color(color);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, rect.center_x(), rect.center_y());
        coord.register_on_layer(wid.as_str(), rect, Sense::CLICK | Sense::HOVER, layer_id);
    }
    32.0
}

fn draw_toggle(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    id: &str,
    label: &str,
    on: bool,
    tt: &ToolbarTheme,
) -> f64 {
    let rect = WidgetRect::new(x, y, w, 24.0);
    hover_fill(ctx, coord, id, rect, tt);
    let mark = WidgetRect::new(x + 4.0, y + 4.0, 16.0, 16.0);
    ctx.set_stroke_color(&tt.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(mark.x, mark.y, mark.width, mark.height, 3.0);
    if on {
        ctx.set_fill_color(&tt.accent);
        ctx.fill_rounded_rect(mark.x + 3.0, mark.y + 3.0, 10.0, 10.0, 2.0);
    }
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&tt.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, x + 26.0, rect.center_y());
    coord.register_on_layer(id, rect, Sense::CLICK | Sense::HOVER, layer_id);
    26.0
}

fn draw_chips(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    members: &[String],
    hits: &[mlc_messenger_model::UserEntry],
    tt: &ToolbarTheme,
) {
    let mut cx = x;
    for (index, id) in members.iter().enumerate() {
        let label = hits
            .iter()
            .find(|h| h.user_id.as_str() == id)
            .map(|h| h.display_name.as_str())
            .filter(|n| !n.is_empty())
            .unwrap_or(id.as_str());
        ctx.set_font("11px sans-serif");
        let tw = ctx.measure_text(label) + 28.0;
        if cx + tw > x + w {
            break;
        }
        let rect = WidgetRect::new(cx, y, tw, 22.0);
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 11.0);
        ctx.set_fill_color(&tt.item_text);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, rect.x + 8.0, rect.center_y());
        let wid = format!("cloud_overlay:dm:create_chip:{index}");
        coord.register_on_layer(wid.as_str(), rect, Sense::CLICK | Sense::HOVER, layer_id);
        cx += tw + 4.0;
    }
}

fn dialog_buttons(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    submit: &str,
    tt: &ToolbarTheme,
) {
    let btn_w = (w - 8.0) / 2.0;
    draw_text_button(
        ctx, coord, layer_id, "cloud_overlay:dm:create_cancel",
        WidgetRect::new(x, y, btn_w, ROW_H - 6.0), "Cancel", &tt.item_text, tt,
    );
    draw_action_row(
        ctx, coord, layer_id, "cloud_overlay:dm:create_submit",
        WidgetRect::new(x + btn_w + 8.0, y, btn_w, ROW_H - 6.0), submit, tt,
    );
}

#[allow(clippy::too_many_arguments)]
fn dialog_field(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    widget_id: &str,
    field_id: &str,
    placeholder: &str,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    out_rect: &mut Option<WidgetRect>,
    out_chars: &mut Vec<f64>,
) -> f64 {
    let (rect, chars) = draw_field_shaped(
        ctx, coord, layer_id, widget_id, field_id, WidgetRect::new(x, y, w, FIELD_H),
        placeholder, ft, tt, current_time_ms, 6.0, 8.0,
    );
    *out_rect = Some(rect);
    *out_chars = chars;
    FIELD_H + FIELD_GAP
}

fn dialog_title(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    title: &str,
    tt: &ToolbarTheme,
) -> f64 {
    ctx.set_font("600 13px sans-serif");
    ctx.set_fill_color(&tt.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(title, x + 12.0, y + 14.0);
    let close = WidgetRect::new(x + w - 28.0, y, 24.0, 24.0);
    let id = if title == "Send to chat" {
        "cloud_overlay:dm:send_chart_cancel"
    } else {
        "cloud_overlay:dm:create_cancel"
    };
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Center);
    ctx.fill_text("\u{2715}", close.center_x(), close.center_y());
    coord.register_on_layer(id, close, Sense::CLICK | Sense::HOVER, layer_id);
    32.0
}

fn centered(x: f64, y: f64, w: f64, h: f64, panel_h: f64) -> (f64, f64, f64) {
    let pw = PANEL_W.min(w - 24.0).max(160.0);
    let px = x + (w - pw) / 2.0;
    let py = y + ((h - panel_h) / 2.0).max(8.0);
    (px, py, pw)
}

fn paint_scrim(ctx: &mut dyn RenderContext, x: f64, y: f64, w: f64, h: f64) {
    ctx.set_global_alpha(0.45);
    ctx.set_fill_color("#000000");
    ctx.fill_rect(x, y, w, h);
    ctx.set_global_alpha(1.0);
}

fn paint_panel(ctx: &mut dyn RenderContext, x: f64, y: f64, w: f64, h: f64, tt: &ToolbarTheme) {
    ctx.set_fill_color(&tt.dropdown_bg);
    ctx.fill_rounded_rect(x, y, w, h, 8.0);
    ctx.set_stroke_color(&tt.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x, y, w, h, 8.0);
}

fn hover_fill(ctx: &mut dyn RenderContext, coord: &InputCoordinator, id: &str, rect: WidgetRect, tt: &ToolbarTheme) {
    if coord.is_hovered(&WidgetId::from(id)) {
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 4.0);
    }
}

fn label_left(ctx: &mut dyn RenderContext, rect: &WidgetRect, text: &str, color: &str) {
    ctx.set_font("12.5px sans-serif");
    ctx.set_fill_color(color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    let fit = crate::ui::widgets::fit_text_to_width(ctx, text, (rect.width - 12.0).max(0.0));
    ctx.fill_text(&fit, rect.x + 6.0, rect.center_y());
}

fn plain_line(ctx: &mut dyn RenderContext, x: f64, y: f64, w: f64, text: &str, color: &str) {
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    let fit = crate::ui::widgets::fit_text_to_width(ctx, text, w);
    ctx.fill_text(&fit, x, y + 10.0);
}

fn section_label(ctx: &mut dyn RenderContext, x: f64, y: f64, text: &str, tt: &ToolbarTheme) {
    ctx.set_font("600 11px sans-serif");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(text, x, y + 8.0);
}

fn square_button(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    id: &str,
    rect: WidgetRect,
    glyph: &str,
    tt: &ToolbarTheme,
) {
    hover_fill(ctx, coord, id, rect, tt);
    ctx.set_font("13px sans-serif");
    ctx.set_fill_color(&tt.item_text);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(glyph, rect.center_x(), rect.center_y());
    coord.register_on_layer(id, rect, Sense::CLICK | Sense::HOVER, layer_id);
}

fn draw_text_button(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    id: &str,
    rect: WidgetRect,
    label: &str,
    color: &str,
    tt: &ToolbarTheme,
) {
    hover_fill(ctx, coord, id, rect, tt);
    ctx.set_font("600 12.5px sans-serif");
    ctx.set_fill_color(color);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, rect.center_x(), rect.center_y());
    coord.register_on_layer(id, rect, Sense::CLICK | Sense::HOVER, layer_id);
}

