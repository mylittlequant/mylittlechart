//! Clicks for the pencil menu, create/join dialogs, the folder editor,
//! the paperclip, and "Send to chat". The painters live in
//! `mlc-app-layout`; this module only turns those widget ids into
//! `MessengerIntent`s. A false `Capabilities` flag hides the control, so
//! a stale click on a hidden action is a no-op.

use mlc_app_layout::ui::modal_settings::{CommunityTab, FolderEditor, MessengerDialog, PendingChartSend};
use mlc_messenger_model::{
    Capabilities, CreateRoomRequest, EventId, Folder, FolderExclude, FolderIcon, FolderInclude, MessengerIntent,
    RoomId, RoomKind, RoomVisibility, TagName, UserId,
};
use uzor::WidgetId;

use crate::text_input::{
    DM_CHANNEL_TITLE, DM_CHANNEL_TOPIC, DM_FOLDER_NAME, DM_GROUP_MEMBER, DM_GROUP_TITLE, DM_JOIN_QUERY, DM_PEER,
};
use crate::ChartApp;

const HIT_CAP: usize = 6;
const JOIN_CAP: usize = 12;
const ROOM_CAP: usize = 40;
const ARCHIVED_TAG: &str = "u.mlc.archived";

impl ChartApp {
    /// `true` when `action` belonged to a dialog, the folder editor, the
    /// paperclip, or a chart card. A click anywhere else closes the pencil
    /// menu and the paperclip menu and returns `false`, so room navigation
    /// still runs. Group, channel, join, the folder editor, and the
    /// send-chart picker stay up until their own Cancel.
    pub(crate) fn dm_dialogs_click(&mut self, action: &str) -> bool {
        if !self.panel_app.community_state.messenger_external {
            return false;
        }
        if self.dialog_action(action) {
            return true;
        }
        let st = &mut self.panel_app.community_state;
        if matches!(st.messenger_dialog, Some(MessengerDialog::Pencil)) {
            st.messenger_dialog = None;
        }
        st.messenger_attach_open = false;
        false
    }

    /// Opens Messages on a room picker for the chart that is open now.
    /// Does not enqueue: the card goes out when a room row is clicked.
    /// Does not create a share.
    pub(crate) fn messenger_begin_send_chart(&mut self) {
        if !self.panel_app.community_state.messenger_chart_cards {
            return;
        }
        if !self.panel_app.user_settings_state.account_signed_in {
            self.panel_app.cloud_login_state.open();
            return;
        }
        let Some(card) = self.active_chart_card() else { return };
        if !self.panel_app.community_state.is_open {
            self.panel_app.community_state.open();
        }
        self.panel_app.community_state.messenger_send_chart = Some(card);
        self.panel_app.community_state.messenger_dialog = None;
        self.panel_app.community_state.messenger_attach_open = false;
        self.panel_app.community_state.set_tab(CommunityTab::Messages);
        self.panel_app.toolbar_state.open_dropdown_id = None;
    }

    fn dialog_action(&mut self, action: &str) -> bool {
        if let Some(rest) = action.strip_prefix("create_member:") {
            self.add_group_member(rest);
            return true;
        }
        if let Some(rest) = action.strip_prefix("create_chip:") {
            self.remove_group_member(rest);
            return true;
        }
        if let Some(rest) = action.strip_prefix("join_room:") {
            self.join_public_room(rest);
            return true;
        }
        if let Some(rest) = action.strip_prefix("folder_edit_pick:") {
            self.pick_folder(rest);
            return true;
        }
        if let Some(rest) = action.strip_prefix("folder_edit_up:") {
            self.reorder_custom_folder(rest, true);
            return true;
        }
        if let Some(rest) = action.strip_prefix("folder_edit_down:") {
            self.reorder_custom_folder(rest, false);
            return true;
        }
        if let Some(rest) = action.strip_prefix("folder_edit_icon:") {
            self.set_folder_icon(rest);
            return true;
        }
        if let Some(rest) = action.strip_prefix("folder_edit_room:") {
            self.toggle_folder_room(rest);
            return true;
        }
        if let Some(room_id) = action.strip_prefix("send_chart_to:") {
            self.send_pending_chart(room_id);
            return true;
        }
        if let Some(token) = action.strip_prefix("card_open:") {
            if share_token_clickable(token) {
                self.request_open_share(token);
            }
            return true;
        }
        if let Some(token) = action.strip_prefix("card_clone:") {
            if share_token_clickable(token) {
                self.request_clone_share(token);
            }
            return true;
        }
        if let Some(event_id) = action.strip_prefix("retry:") {
            self.retry_failed_send(event_id);
            return true;
        }
        match action {
            "group_title_input" | "group_member_input" | "channel_title_input" | "channel_topic_input"
            | "join_query_input" | "folder_name_input" => true,
            "new_conversation_toggle" => {
                self.toggle_pencil();
                true
            }
            "create_menu_message" => {
                self.open_new_message();
                true
            }
            "create_menu_group" => {
                self.open_group_dialog();
                true
            }
            "create_menu_channel" => {
                self.open_channel_dialog();
                true
            }
            "create_menu_join" => {
                self.open_join_dialog();
                true
            }
            "create_menu_folder" => {
                self.open_new_folder_editor();
                true
            }
            "create_cancel" => {
                self.close_create_dialog();
                true
            }
            "create_submit" => {
                self.submit_create();
                true
            }
            "create_members_can_add" => {
                self.toggle_members_can_add();
                true
            }
            "folder:folders" => {
                self.open_folders_screen();
                true
            }
            "folder_edit_inc:dms" => {
                self.flip_editor(|editor| editor.all_dms = !editor.all_dms);
                true
            }
            "folder_edit_inc:groups" => {
                self.flip_editor(|editor| editor.all_groups = !editor.all_groups);
                true
            }
            "folder_edit_inc:channels" => {
                self.flip_editor(|editor| editor.all_channels = !editor.all_channels);
                true
            }
            "folder_edit_exc:muted" => {
                self.flip_editor(|editor| editor.exclude_muted = !editor.exclude_muted);
                true
            }
            "folder_edit_exc:read" => {
                self.flip_editor(|editor| editor.exclude_read = !editor.exclude_read);
                true
            }
            "folder_edit_exc:archived" => {
                self.flip_editor(|editor| editor.exclude_archived = !editor.exclude_archived);
                true
            }
            "folder_edit_save" => {
                self.save_folder();
                true
            }
            "folder_edit_cancel" => {
                self.panel_app.community_state.messenger_folder_editor = None;
                self.blur_fields();
                true
            }
            "folder_edit_delete" => {
                self.delete_folder();
                true
            }
            "attach" => {
                let st = &mut self.panel_app.community_state;
                if st.messenger_chart_cards {
                    st.messenger_attach_open = !st.messenger_attach_open;
                } else {
                    st.messenger_attach_open = false;
                }
                true
            }
            "attach_chart" => {
                self.attach_current_chart();
                true
            }
            "send_chart_cancel" => {
                self.panel_app.community_state.messenger_send_chart = None;
                true
            }
            _ => false,
        }
    }

    fn caps(&self) -> Capabilities {
        self.panel_app.messenger_capabilities().unwrap_or_default()
    }

    fn toggle_pencil(&mut self) {
        let open = matches!(self.panel_app.community_state.messenger_dialog, Some(MessengerDialog::Pencil));
        if open {
            self.panel_app.community_state.messenger_dialog = None;
            return;
        }
        self.panel_app.community_state.messenger_folder_editor = None;
        self.panel_app.community_state.messenger_attach_open = false;
        self.panel_app.community_state.dm.new_conversation_open = false;
        self.panel_app.community_state.messenger_dialog = Some(MessengerDialog::Pencil);
    }

    fn open_new_message(&mut self) {
        self.panel_app.community_state.messenger_dialog = None;
        self.panel_app.community_state.messenger_folder_editor = None;
        self.panel_app.community_state.dm.new_conversation_open = true;
        self.focus_field(DM_PEER);
    }

    fn open_group_dialog(&mut self) {
        if !self.caps().groups {
            return;
        }
        self.panel_app.community_state.messenger_folder_editor = None;
        self.panel_app.community_state.messenger_attach_open = false;
        self.panel_app.community_state.dm.new_conversation_open = false;
        self.panel_app.community_state.messenger_dialog = Some(MessengerDialog::Group {
            members: Vec::new(),
            members_can_add: true,
        });
        self.set_field(DM_GROUP_TITLE, "");
        self.set_field(DM_GROUP_MEMBER, "");
        self.focus_field(DM_GROUP_TITLE);
    }

    fn open_channel_dialog(&mut self) {
        if !self.caps().channels {
            return;
        }
        self.panel_app.community_state.messenger_folder_editor = None;
        self.panel_app.community_state.messenger_attach_open = false;
        self.panel_app.community_state.dm.new_conversation_open = false;
        self.panel_app.community_state.messenger_dialog = Some(MessengerDialog::Channel);
        self.set_field(DM_CHANNEL_TITLE, "");
        self.set_field(DM_CHANNEL_TOPIC, "");
        self.focus_field(DM_CHANNEL_TITLE);
    }

    fn open_join_dialog(&mut self) {
        if !self.caps().channels {
            return;
        }
        self.panel_app.community_state.messenger_folder_editor = None;
        self.panel_app.community_state.messenger_attach_open = false;
        self.panel_app.community_state.dm.new_conversation_open = false;
        self.panel_app.community_state.messenger_dialog = Some(MessengerDialog::Join);
        self.set_field(DM_JOIN_QUERY, "");
        self.focus_field(DM_JOIN_QUERY);
    }

    fn open_folders_screen(&mut self) {
        if !self.caps().folders {
            return;
        }
        if self.panel_app.community_state.messenger_folder_editor.is_some() {
            return;
        }
        self.open_new_folder_editor();
    }

    fn open_new_folder_editor(&mut self) {
        if !self.caps().folders {
            return;
        }
        self.panel_app.community_state.messenger_dialog = None;
        self.panel_app.community_state.messenger_attach_open = false;
        self.panel_app.community_state.dm.new_conversation_open = false;
        self.panel_app.community_state.messenger_folder_editor = Some(blank_folder_editor());
        self.set_field(DM_FOLDER_NAME, "");
        self.focus_field(DM_FOLDER_NAME);
    }

    fn close_create_dialog(&mut self) {
        self.panel_app.community_state.messenger_dialog = None;
        self.blur_fields();
    }

    fn submit_create(&mut self) {
        match self.panel_app.community_state.messenger_dialog.clone() {
            Some(MessengerDialog::Group { members, members_can_add }) => {
                self.submit_group(members, members_can_add);
            }
            Some(MessengerDialog::Channel) => self.submit_channel(),
            _ => {}
        }
    }

    fn submit_group(&mut self, members: Vec<String>, members_can_add: bool) {
        if members.is_empty() {
            return;
        }
        let title_raw = self.field_text(DM_GROUP_TITLE);
        let title = if title_raw.is_empty() { None } else { Some(title_raw) };
        self.panel_app.messenger_enqueue(MessengerIntent::CreateRoom(CreateRoomRequest {
            kind: RoomKind::Group,
            title,
            description: None,
            member_ids: members.iter().map(|id| UserId::from(id.as_str())).collect(),
            members_can_add,
            visibility: RoomVisibility::Private,
        }));
        self.close_create_dialog();
    }

    fn submit_channel(&mut self) {
        let title = self.field_text(DM_CHANNEL_TITLE);
        if title.is_empty() {
            return;
        }
        let topic = self.field_text(DM_CHANNEL_TOPIC);
        let description = if topic.is_empty() { None } else { Some(topic) };
        self.panel_app.messenger_enqueue(MessengerIntent::CreateRoom(CreateRoomRequest {
            kind: RoomKind::Channel,
            title: Some(title),
            description,
            member_ids: Vec::new(),
            members_can_add: false,
            visibility: RoomVisibility::Public,
        }));
        self.close_create_dialog();
    }

    fn add_group_member(&mut self, index_str: &str) {
        let Ok(index) = index_str.parse::<usize>() else { return };
        if index >= HIT_CAP {
            return;
        }
        let Some(id) = self.panel_app.messenger_view().and_then(|provider| {
            provider.user_search().get(index).map(|hit| hit.user_id.as_str().to_string())
        }) else {
            return;
        };
        if let Some(MessengerDialog::Group { members, .. }) = &mut self.panel_app.community_state.messenger_dialog {
            if !members.iter().any(|member| member == &id) {
                members.push(id);
            }
        }
    }

    fn remove_group_member(&mut self, index_str: &str) {
        let Ok(index) = index_str.parse::<usize>() else { return };
        if let Some(MessengerDialog::Group { members, .. }) = &mut self.panel_app.community_state.messenger_dialog {
            if index < members.len() {
                members.remove(index);
            }
        }
    }

    fn toggle_members_can_add(&mut self) {
        if let Some(MessengerDialog::Group { members_can_add, .. }) =
            &mut self.panel_app.community_state.messenger_dialog
        {
            *members_can_add = !*members_can_add;
        }
    }

    fn join_public_room(&mut self, index_str: &str) {
        let Ok(index) = index_str.parse::<usize>() else { return };
        if index >= JOIN_CAP {
            return;
        }
        let Some(room) = self.panel_app.messenger_view().and_then(|provider| provider.public_rooms().into_iter().nth(index))
        else {
            return;
        };
        if room.joined {
            return;
        }
        self.panel_app.messenger_enqueue(MessengerIntent::JoinRoom { room: room.room_id });
        self.close_create_dialog();
    }

    fn flip_editor(&mut self, flip: impl FnOnce(&mut FolderEditor)) {
        if let Some(editor) = &mut self.panel_app.community_state.messenger_folder_editor {
            flip(editor);
        }
    }

    fn set_folder_icon(&mut self, index_str: &str) {
        let Ok(index) = index_str.parse::<u8>() else { return };
        if index > 5 {
            return;
        }
        if let Some(editor) = &mut self.panel_app.community_state.messenger_folder_editor {
            editor.icon_idx = index;
        }
    }

    fn toggle_folder_room(&mut self, index_str: &str) {
        let Ok(index) = index_str.parse::<usize>() else { return };
        if index >= ROOM_CAP {
            return;
        }
        let Some(room_id) = self.panel_app.messenger_view().and_then(|provider| {
            provider.rooms().into_iter().nth(index).map(|room| room.id.as_str().to_string())
        }) else {
            return;
        };
        if let Some(editor) = &mut self.panel_app.community_state.messenger_folder_editor {
            if let Some(pos) = editor.rooms.iter().position(|id| id == &room_id) {
                editor.rooms.remove(pos);
            } else {
                editor.rooms.push(room_id);
            }
        }
    }

    fn pick_folder(&mut self, tag: &str) {
        let Some(folder) = self.panel_app.messenger_view().and_then(|provider| {
            provider.folders().into_iter().find(|folder| folder.id.as_ref().map(|id| id.as_str()) == Some(tag))
        }) else {
            return;
        };
        let name = folder.name.clone();
        self.panel_app.community_state.messenger_folder_editor = Some(editor_from_folder(&folder));
        self.set_field(DM_FOLDER_NAME, &name);
        self.focus_field(DM_FOLDER_NAME);
    }

    fn save_folder(&mut self) {
        let Some(editor) = self.panel_app.community_state.messenger_folder_editor.clone() else { return };
        let name = self.field_text(DM_FOLDER_NAME);
        if name.is_empty() {
            return;
        }
        let creating = editor.tag.is_none();
        let tag = editor.tag.clone().unwrap_or_else(|| format!("u.mlc.{}", unix_ms()));
        if !custom_folder_tag_ok(&tag) {
            return;
        }
        let folders = self.panel_app.messenger_view().map(|provider| provider.folders()).unwrap_or_default();
        let order = if creating {
            folders.iter().filter(|folder| folder.id.is_some()).map(|folder| folder.order).reduce(f64::max).map(|max| max + 1.0).unwrap_or(10.0)
        } else {
            folders
                .iter()
                .find(|folder| folder.id.as_ref().map(|id| id.as_str()) == Some(tag.as_str()))
                .map(|folder| folder.order)
                .unwrap_or(10.0)
        };
        let folder = Folder {
            id: Some(TagName::from(tag.as_str())),
            name,
            icon: folder_icon(editor.icon_idx),
            include: FolderInclude {
                all_dms: editor.all_dms,
                all_groups: editor.all_groups,
                all_channels: editor.all_channels,
                rooms: editor.rooms.iter().map(|id| RoomId::from(id.as_str())).collect(),
            },
            exclude: FolderExclude {
                muted: editor.exclude_muted,
                read: editor.exclude_read,
                archived: editor.exclude_archived,
            },
            order,
        };
        let intent = if creating { MessengerIntent::CreateFolder(folder) } else { MessengerIntent::UpdateFolder(folder) };
        self.panel_app.messenger_enqueue(intent);
        self.panel_app.community_state.messenger_folder_editor = None;
        self.blur_fields();
    }

    fn delete_folder(&mut self) {
        let tag = self.panel_app.community_state.messenger_folder_editor.as_ref().and_then(|editor| editor.tag.clone());
        let Some(tag) = tag else { return };
        if !custom_folder_tag_ok(&tag) {
            return;
        }
        self.panel_app.messenger_enqueue(MessengerIntent::DeleteFolder(TagName::from(tag.as_str())));
        self.panel_app.community_state.messenger_folder_editor = None;
        self.blur_fields();
    }

    fn reorder_custom_folder(&mut self, tag: &str, up: bool) {
        let Some(folders) = self.panel_app.messenger_view().map(|provider| provider.folders()) else { return };
        let mut custom: Vec<(String, f64)> = folders
            .iter()
            .filter_map(|folder| folder.id.as_ref().map(|id| (id.as_str().to_string(), folder.order)))
            .collect();
        custom.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let Some(index) = custom.iter().position(|(id, _)| id == tag) else { return };
        let neighbor = if up {
            index.checked_sub(1)
        } else if index + 1 < custom.len() {
            Some(index + 1)
        } else {
            None
        };
        let Some(neighbor) = neighbor else { return };
        custom.swap(index, neighbor);
        let pairs = custom
            .into_iter()
            .enumerate()
            .map(|(i, (id, _))| (TagName::from(id.as_str()), (i as f64 + 1.0) * 10.0))
            .collect();
        self.panel_app.messenger_enqueue(MessengerIntent::ReorderFolders(pairs));
    }

    fn attach_current_chart(&mut self) {
        self.panel_app.community_state.messenger_attach_open = false;
        if !self.panel_app.community_state.messenger_chart_cards {
            return;
        }
        let Some(room) = self.panel_app.community_state.messenger_room.clone() else { return };
        let Some(card) = self.active_chart_card() else { return };
        self.enqueue_chart_card(room, card);
    }

    fn send_pending_chart(&mut self, room_id: &str) {
        let Some(card) = self.panel_app.community_state.messenger_send_chart.clone() else { return };
        self.panel_app.community_state.messenger_send_chart = None;
        if room_id.is_empty() {
            return;
        }
        self.enqueue_chart_card(RoomId::from(room_id), card);
    }

    fn enqueue_chart_card(&mut self, room: RoomId, card: PendingChartSend) {
        if card.symbol.is_empty() {
            return;
        }
        let body = format!("{} {}", card.symbol, card.timeframe);
        self.panel_app.messenger_enqueue(MessengerIntent::SendChartCard {
            room,
            body,
            symbol: card.symbol,
            timeframe: card.timeframe,
            venue: card.venue,
            note: String::new(),
            allow_forward: false,
            share_token: card.share_token,
        });
    }

    fn retry_failed_send(&mut self, event_id: &str) {
        if event_id.is_empty() {
            return;
        }
        let Some(room) = self.panel_app.community_state.messenger_room.clone() else { return };
        self.panel_app.messenger_enqueue(MessengerIntent::RetrySend {
            room,
            local_id: EventId::from(event_id),
        });
    }

    /// The chart in the active preset's first window. `None` when that
    /// window has no symbol. The share token, if the chart already has
    /// one, is read before the preset lock.
    fn active_chart_card(&self) -> Option<PendingChartSend> {
        let share_token = self.active_share_token();
        let preset_id = self.panel_app.active_preset_id.clone();
        let presets = self.core.presets();
        let window = presets.get(&preset_id)?.windows.first()?;
        if window.symbol.is_empty() {
            return None;
        }
        Some(PendingChartSend {
            symbol: window.symbol.clone(),
            timeframe: window.timeframe.name.clone(),
            venue: window.exchange.clone(),
            note: String::new(),
            share_token,
        })
    }

    fn field_text(&self, field_id: &str) -> String {
        let wid = WidgetId::from(field_id);
        self.input_coordinator.borrow().text_fields().text(&wid).trim().to_string()
    }

    fn set_field(&mut self, field_id: &str, text: &str) {
        let wid = WidgetId::from(field_id);
        self.input_coordinator.borrow_mut().text_fields_mut().set_text(&wid, text);
    }

    fn focus_field(&mut self, field_id: &'static str) {
        let wid = WidgetId::from(field_id);
        let mut coord = self.input_coordinator.borrow_mut();
        let fields = coord.text_fields_mut();
        fields.begin_edit(&wid);
        fields.focus(field_id);
    }

    fn blur_fields(&mut self) {
        self.input_coordinator.borrow_mut().text_fields_mut().blur();
    }
}

fn blank_folder_editor() -> FolderEditor {
    FolderEditor {
        tag: None,
        icon_idx: 1,
        all_dms: false,
        all_groups: false,
        all_channels: false,
        rooms: Vec::new(),
        exclude_muted: false,
        exclude_read: false,
        exclude_archived: false,
    }
}

fn editor_from_folder(folder: &Folder) -> FolderEditor {
    FolderEditor {
        tag: folder.id.as_ref().map(|tag| tag.as_str().to_string()),
        icon_idx: match folder.icon {
            FolderIcon::Candles => 0,
            FolderIcon::Folder => 1,
            FolderIcon::Group => 2,
            FolderIcon::Channel => 3,
            FolderIcon::Bookmark => 4,
            FolderIcon::Person => 5,
        },
        all_dms: folder.include.all_dms,
        all_groups: folder.include.all_groups,
        all_channels: folder.include.all_channels,
        rooms: folder.include.rooms.iter().map(|room| room.as_str().to_string()).collect(),
        exclude_muted: folder.exclude.muted,
        exclude_read: folder.exclude.read,
        exclude_archived: folder.exclude.archived,
    }
}

fn folder_icon(idx: u8) -> FolderIcon {
    match idx {
        0 => FolderIcon::Candles,
        1 => FolderIcon::Folder,
        2 => FolderIcon::Group,
        3 => FolderIcon::Channel,
        4 => FolderIcon::Bookmark,
        _ => FolderIcon::Person,
    }
}

fn custom_folder_tag_ok(tag: &str) -> bool {
    tag.starts_with("u.") && tag != ARCHIVED_TAG
}

fn share_token_clickable(token: &str) -> bool {
    !token.is_empty() && !token.contains(':') && !token.contains(' ')
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}
