//! Centralised text-input dispatcher.
//!
//! Replaces the 30+ inline `if X.is_open() { text_fields.on_char(ch); ... }`
//! blocks that used to live in `mlc-app/src/input/mod.rs::on_char_input` and
//! `on_key_press`.  uzor's `TextFieldStore` already routes char/key input to
//! whichever WidgetId is `focused()`; we only need to apply the per-field
//! side-effects (search-query updates, modal close on Commit, focus chain on
//! Tab, etc.).
//!
//! Side-effects are matched on `focused_id.as_str()` against the constants in
//! `super::*` (HEX_COLOR / CHART_BROWSER_SEARCH / NEW_PASSPHRASE / etc.).

use crate::ChartApp;
use crate::input::to_uzor_key;
use crate::input::KeyPress;
use uzor::input::TextAction;
use uzor::types::WidgetId;

// On native, gate4agent is an external crate.
// On wasm32 it is absent from the dep graph — use the in-tree stub re-exported
// by lib.rs as `crate::gate4agent`.
#[cfg(not(target_arch = "wasm32"))]
use gate4agent;
#[cfg(target_arch = "wasm32")]
use crate::agent as gate4agent;

impl ChartApp {
    /// Centralised char-input dispatch.  Called by `on_char_input` in mod.rs.
    pub(crate) fn dispatch_text_char(&mut self, ch: char) {
        // 1. Profile-manager gate: while the profile manager is shown, only
        //    passphrase / recovery / name fields may receive char input.
        if self.profile_manager_blocks_text_input() {
            return;
        }

        // 2. Chart browser hard-block: while open, chars only reach the search
        //    field; if that field is not focused, drop the char.  This preserves
        //    the original on_char_input semantics (line 5953-5967).
        if self.panel_app.chart_browser.is_open {
            let cb_id = uzor::WidgetId::from(crate::text_input::CHART_BROWSER_SEARCH);
            if !self.input_coordinator.borrow().text_fields().is_focused(&cb_id) {
                return;
            }
        }

        // 3. Single call into TextFieldStore — store routes to focused field.
        let (focused_id, action) = {
            let mut ic = self.input_coordinator.borrow_mut();
            let tf = ic.text_fields_mut();
            let focused = tf.focused().cloned();
            let action = tf.on_char(ch);
            (focused, action)
        };

        // 4. If nothing focused, no field consumed the char — nothing to do.
        let Some(focused_id) = focused_id else { return };

        // 5. Per-field side-effects.
        self.apply_text_action(&focused_id, ch, action);
    }

    /// Early key dispatch: covers fields that run BEFORE global undo/redo in on_key_press.
    ///
    /// Returns true if the key was routed to a text field (caller should return early
    /// to prevent the key from reaching global shortcuts).
    ///
    /// Covers (in order): ALERT_TG_TOKEN, PROFILE_RENAME, NEW_PASSPHRASE,
    /// NEW_PROFILE_NAME, RECOVERY_KEY, CONFIRM_PASSPHRASE, BAR_MODE_PARAM,
    /// TF_FAVORITE_INPUT, PARAM_FAVORITE_INPUT, HEX_COLOR, AGENT_PTY, AGENT_CHAT.
    pub(crate) fn dispatch_text_key_early(&mut self, key: &KeyPress) -> bool {
        use crate::text_input::*;

        // Profile-manager gate — same as on_char_input.
        if self.profile_manager_blocks_text_input() {
            return false;
        }

        // Identify focused field without calling on_key yet.
        let focused_id = {
            let ic = self.input_coordinator.borrow();
            ic.text_fields().focused().cloned()
        };
        let Some(focused_id) = focused_id else { return false };

        // Only route early fields; late fields (chart_browser, presets, settings, search)
        // are handled after global undo/redo by dispatch_text_key_late.
        let is_early_field = matches!(
            focused_id.as_str(),
            ALERT_TG_TOKEN
                | PROFILE_RENAME
                | NEW_PASSPHRASE
                | NEW_PROFILE_NAME
                | RECOVERY_KEY
                | CONFIRM_PASSPHRASE
                | BAR_MODE_PARAM
                | TF_FAVORITE_INPUT
                | PARAM_FAVORITE_INPUT
                | HEX_COLOR
                | AGENT_PTY
                | AGENT_CHAT
        );
        if !is_early_field {
            return false;
        }

        // AGENT_CHAT: Paste falls through (original line 6622).
        if focused_id.as_str() == AGENT_CHAT {
            if let KeyPress::Paste(_) = key {
                // fall through — don't consume
                // (original code fell through to generic routing, which also
                // did nothing for AGENT_CHAT Paste in on_key_press)
            }
        }

        // Convert KeyPress → uzor KeyPress.
        let uzor_key = match to_uzor_key(key) {
            Some(k) => k,
            None => return true, // field was focused; consume even if no mapping
        };

        // Single call into TextFieldStore.
        let action = {
            let mut ic = self.input_coordinator.borrow_mut();
            ic.text_fields_mut().on_key(uzor_key)
        };

        // Per-field side-effects.
        // AGENT_CHAT needs special handling: after on_key, sync the input buffer.
        if focused_id.as_str() == AGENT_CHAT {
            self.text_action_agent_chat_key();
        } else {
            self.apply_text_action(&focused_id, '\0', action);
        }

        true
    }

    /// Late key dispatch: covers fields that run AFTER global undo/redo in on_key_press.
    ///
    /// Returns true if the key was routed to a text field.
    ///
    /// Covers (in order): CHART_BROWSER_SEARCH, WATCHLIST_SEARCH, WATCHLIST_GROUP_NAME,
    /// PRESET_NAME_INPUT, template name fields, settings sub-fields, search overlay.
    pub(crate) fn dispatch_text_key_late(&mut self, key: &KeyPress) -> bool {
        use crate::text_input::*;

        // Identify focused field.
        let focused_id = {
            let ic = self.input_coordinator.borrow();
            ic.text_fields().focused().cloned()
        };
        let Some(focused_id) = focused_id else { return false };

        let is_late_field = matches!(
            focused_id.as_str(),
            CHART_BROWSER_SEARCH
                | WATCHLIST_SEARCH
                | WATCHLIST_GROUP_NAME
                | PRESET_NAME_INPUT
                | PRIMITIVE_SETTINGS_TEMPLATE_NAME
                | INDICATOR_SETTINGS_TEMPLATE_NAME
                | CHART_SETTINGS_TEMPLATE_NAME
                | COMPARE_SETTINGS_TEMPLATE_NAME
                | PRIMITIVE_SETTINGS_FIELD
                | INDICATOR_SETTINGS_FIELD
                | CHART_SETTINGS_FIELD
                | COMPARE_SETTINGS_FIELD
                | SYMBOL_SEARCH
                | INDICATOR_SEARCH
                | COMPARE_SEARCH
        );
        if !is_late_field {
            return false;
        }

        // Chart browser guard: if open, always consume the key (original return semantics).
        if self.panel_app.chart_browser.is_open && focused_id.as_str() != CHART_BROWSER_SEARCH {
            return true;
        }

        let uzor_key = match to_uzor_key(key) {
            Some(k) => k,
            None => return true,
        };

        let action = {
            let mut ic = self.input_coordinator.borrow_mut();
            ic.text_fields_mut().on_key(uzor_key)
        };

        self.apply_text_action(&focused_id, '\0', action);
        true
    }

    fn apply_text_action(&mut self, id: &WidgetId, ch: char, action: TextAction) {
        use crate::text_input::*;
        match id.as_str() {
            HEX_COLOR                        => self.text_action_hex_color(action),
            CHART_BROWSER_SEARCH             => self.text_action_chart_browser(action),
            WATCHLIST_SEARCH                 => self.text_action_watchlist_search(action),
            WATCHLIST_GROUP_NAME             => self.text_action_wl_group_name(action),
            PRESET_NAME_INPUT                => self.text_action_preset_name(action),
            NEW_PASSPHRASE                   => self.text_action_new_passphrase(ch, action),
            CONFIRM_PASSPHRASE               => self.text_action_confirm_passphrase(ch, action),
            RECOVERY_KEY                     => self.text_action_recovery_key(ch, action),
            NEW_PROFILE_NAME                 => self.text_action_new_profile_name(ch, action),
            PROFILE_RENAME                   => self.text_action_profile_rename(action),
            SYMBOL_SEARCH | INDICATOR_SEARCH | COMPARE_SEARCH
                                             => self.text_action_search_overlay(action),
            AGENT_CHAT                       => self.text_action_agent_chat(ch, action),
            AGENT_PTY                        => self.text_action_agent_pty(action),
            ALERT_TG_TOKEN                   => self.text_action_alert_token(ch, action),
            BAR_MODE_PARAM                   => self.text_action_bar_mode_param(action),
            TF_FAVORITE_INPUT                => self.text_action_tf_favorite_input(action),
            PARAM_FAVORITE_INPUT             => self.text_action_param_favorite_input(action),
            PRIMITIVE_SETTINGS_FIELD         => self.text_action_prim_settings_field(action),
            INDICATOR_SETTINGS_FIELD         => self.text_action_ind_settings_field(action),
            CHART_SETTINGS_FIELD             => self.text_action_chart_settings_field(action),
            COMPARE_SETTINGS_FIELD           => self.text_action_cmp_settings_field(action),
            PRIMITIVE_SETTINGS_TEMPLATE_NAME
            | INDICATOR_SETTINGS_TEMPLATE_NAME
            | CHART_SETTINGS_TEMPLATE_NAME
            | COMPARE_SETTINGS_TEMPLATE_NAME => self.text_action_template_name(id.as_str(), action),
            _ => {}
        }
    }

    pub(crate) fn profile_manager_blocks_text_input(&self) -> bool {
        let uss = &self.panel_app.user_settings_state;
        uss.show_profile_manager
            && !uss.new_passphrase_focused
            && !uss.new_profile_name_focused
            && !uss.recovery_key_display_focused
            && !uss.confirm_passphrase_focused
    }

    // ── Per-field handlers ──────────────────────────────────────────────────

    fn text_action_hex_color(&mut self, action: TextAction) {
        match action {
            TextAction::Commit(_) | TextAction::Cancel => {
                self.panel_app.primitive_settings_state.color_picker.hex_editing = false;
                self.panel_app.indicator_settings_state.color_picker.hex_editing = false;
                self.panel_app.chart_settings_state.color_picker.hex_editing = false;
                self.panel_app.compare_settings_state.color_picker.hex_editing = false;
                self.panel_app.panel_color_picker.hex_editing = false;
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
            }
            TextAction::TextChanged(ref new_hex) => {
                let pickers: [(&mut mlc_app_layout::ui::color_picker_state::ColorPickerState, &str); 5] = [
                    (&mut self.panel_app.primitive_settings_state.color_picker, "primitive"),
                    (&mut self.panel_app.indicator_settings_state.color_picker, "indicator"),
                    (&mut self.panel_app.chart_settings_state.color_picker, "chart"),
                    (&mut self.panel_app.compare_settings_state.color_picker, "compare"),
                    (&mut self.panel_app.panel_color_picker, "panel"),
                ];
                for (picker, _src) in pickers {
                    if picker.hex_editing {
                        picker.hex_set_text(new_hex);
                        break;
                    }
                }
            }
            TextAction::None => {}
            TextAction::RawInput(_) => {}
        }
    }

    fn text_action_chart_browser(&mut self, action: TextAction) {
        match action {
            TextAction::TextChanged(ref new_text) | TextAction::Commit(ref new_text) => {
                self.panel_app.chart_browser.search_query = new_text.clone();
                self.panel_app.chart_browser.scroll.reset();
            }
            TextAction::Cancel | TextAction::None | TextAction::RawInput(_) => {}
        }
    }

    fn text_action_watchlist_search(&mut self, action: TextAction) {
        match action {
            TextAction::TextChanged(ref new_text) | TextAction::Commit(ref new_text) => {
                self.standalone_panels.watchlist_modal.search_query = new_text.clone();
                self.standalone_panels.watchlist_modal.scroll.reset();
            }
            TextAction::Cancel | TextAction::None | TextAction::RawInput(_) => {}
        }
    }

    fn text_action_wl_group_name(&mut self, action: TextAction) {
        match action {
            TextAction::Commit(name) => {
                let name = name.trim().to_string();
                if !name.is_empty() {
                    use mlc_app_layout::ui::modal_settings::WatchlistGroupNameMode;
                    match self.standalone_panels.wl_group_name_input.mode.clone() {
                        WatchlistGroupNameMode::CreateNew => {
                            let new_id = self.core.watchlists_mut().create_list(name.clone());
                            self.watchlist_ui.active_list_id = new_id;
                            self.watchlist_actions.push(crate::WatchlistAction::CreateList { name: name.clone() });
                            self.watchlists_dirty = true;
                            self.persist_watchlists();
                            eprintln!("[WatchlistGroupName] created new list '{}' id={}", name, new_id);
                        }
                        WatchlistGroupNameMode::Rename(id) => {
                            let found = {
                                let mut wl = self.core.watchlists_mut();
                                if let Some(list) = wl.find_mut(id) {
                                    list.name = name.clone();
                                    true
                                } else {
                                    false
                                }
                            };
                            if found {
                                self.watchlist_actions.push(crate::WatchlistAction::RenameList { id, new_name: name.clone() });
                                self.watchlists_dirty = true;
                                self.persist_watchlists();
                                eprintln!("[WatchlistGroupName] renamed list id={} to '{}'", id, name);
                            }
                        }
                    }
                }
                self.standalone_panels.wl_group_name_input.close();
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
            }
            TextAction::Cancel => {
                self.standalone_panels.wl_group_name_input.close();
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
            }
            TextAction::TextChanged(_) | TextAction::None | TextAction::RawInput(_) => {}
        }
    }

    fn text_action_preset_name(&mut self, action: TextAction) {
        match action {
            TextAction::Commit(name) => {
                if !name.trim().is_empty() {
                    use mlc_app_layout::ui::modal_settings::PresetNameInputMode;
                    match self.panel_app.preset_name_input.mode {
                        PresetNameInputMode::SaveAs => {
                            self.process_chart_out_event(
                                mlc_core::events::ChartOutEvent::SavePreset { name }
                            );
                        }
                        PresetNameInputMode::Rename => {
                            let id = self.panel_app.preset_name_input.rename_preset_id
                                .clone().unwrap_or_default();
                            self.process_chart_out_event(
                                mlc_core::events::ChartOutEvent::RenamePreset { id, new_name: name }
                            );
                        }
                        PresetNameInputMode::NewChart => {
                            self.execute_new_chart_with_name(name);
                        }
                        PresetNameInputMode::CreateIndicatorSet => {
                            self.execute_create_indicator_set(name);
                        }
                    }
                }
                self.panel_app.preset_name_input.close();
                self.panel_app.active_modal = mlc_core::modal::ChartOpenModal::None;
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
            }
            TextAction::Cancel => {
                self.panel_app.preset_name_input.close();
                self.panel_app.active_modal = mlc_core::modal::ChartOpenModal::None;
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
            }
            TextAction::TextChanged(_) | TextAction::None | TextAction::RawInput(_) => {}
        }
    }

    fn text_action_new_passphrase(&mut self, ch: char, action: TextAction) {
        // Tab and Escape are char-level routing in the passphrase field.
        let uss = &mut self.panel_app.user_settings_state;
        match ch {
            '\x09' => {
                // Tab moves to next field in wizard.
                if uss.show_welcome_wizard {
                    self.input_coordinator.borrow_mut().text_fields_mut().blur();
                    let cp_wid = WidgetId::from(crate::text_input::CONFIRM_PASSPHRASE);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(cp_wid);
                    uss.new_passphrase_focused = false;
                    uss.confirm_passphrase_focused = true;
                }
            }
            '\x1b' => {
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
                uss.new_passphrase_focused = false;
            }
            _ => {
                match action {
                    TextAction::Commit(passphrase) => {
                        use mlc_app_layout::ui::modal_settings::ProfileManagerPage;
                        let on_passphrase_page = if uss.show_profile_manager {
                            matches!(
                                uss.profile_manager_page,
                                ProfileManagerPage::UnlockPassphrase | ProfileManagerPage::CreatePassphrase
                            )
                        } else {
                            true
                        };
                        let passphrase = passphrase.trim().to_string();
                        if !passphrase.is_empty() && on_passphrase_page {
                            self.pending_updater_cmd = Some(format!("e2e_setup:{}", passphrase));
                            self.input_coordinator.borrow_mut().text_fields_mut().clear(
                                &WidgetId::from(crate::text_input::NEW_PASSPHRASE)
                            );
                        }
                        self.panel_app.user_settings_state.new_passphrase_focused = false;
                    }
                    _ => {
                        if ch == '\x08' || !ch.is_control() {
                            self.panel_app.user_settings_state.vault_unlock_error = None;
                        }
                    }
                }
            }
        }
    }

    fn text_action_confirm_passphrase(&mut self, ch: char, action: TextAction) {
        let uss = &mut self.panel_app.user_settings_state;
        match ch {
            '\x09' => {
                // Tab cycles to next/first field.
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
                if uss.show_welcome_wizard {
                    let npn_wid = WidgetId::from(crate::text_input::NEW_PROFILE_NAME);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(npn_wid);
                    uss.confirm_passphrase_focused = false;
                    uss.new_profile_name_focused = true;
                } else {
                    let np_wid = WidgetId::from(crate::text_input::NEW_PASSPHRASE);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(np_wid);
                    uss.confirm_passphrase_focused = false;
                    uss.new_passphrase_focused = true;
                }
            }
            '\x1b' => {
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
                uss.confirm_passphrase_focused = false;
            }
            _ => {
                match action {
                    TextAction::Commit(_) => {
                        let np_wid = WidgetId::from(crate::text_input::NEW_PASSPHRASE);
                        let cp_wid = WidgetId::from(crate::text_input::CONFIRM_PASSPHRASE);
                        let passphrase_text = {
                            let ic = self.input_coordinator.borrow();
                            ic.text_fields().text(&np_wid).to_string()
                        };
                        let confirm_text = {
                            let ic = self.input_coordinator.borrow();
                            ic.text_fields().text(&cp_wid).to_string()
                        };
                        use crate::user_manager::profile_manager::MIN_PASSPHRASE_LENGTH;
                        let uss = &mut self.panel_app.user_settings_state;
                        if passphrase_text.len() >= MIN_PASSPHRASE_LENGTH
                            && confirm_text == passphrase_text
                        {
                            uss.set_passphrase_error.clear();
                            uss.confirm_passphrase_focused = false;
                            self.pending_updater_cmd = Some(format!("set_new_passphrase:{}", passphrase_text));
                            eprintln!("[ChartApp] profile_mgr: set_new_passphrase submitted via Enter");
                        } else if confirm_text != passphrase_text {
                            uss.set_passphrase_error = "Passphrases do not match".to_string();
                        }
                    }
                    _ => {
                        if ch == '\x08' || !ch.is_control() {
                            self.panel_app.user_settings_state.set_passphrase_error.clear();
                        }
                    }
                }
            }
        }
    }

    fn text_action_recovery_key(&mut self, ch: char, action: TextAction) {
        let uss = &mut self.panel_app.user_settings_state;
        match ch {
            '\x1b' => {
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
                uss.recovery_key_display_focused = false;
            }
            _ => {
                match action {
                    TextAction::Commit(key_text) => {
                        if key_text.len() >= 40 {
                            uss.vault_unlock_error = None;
                            self.pending_updater_cmd = Some(format!("recovery_unlock:{}", key_text));
                            eprintln!("[ChartApp] profile_mgr: recovery unlock submitted via Enter");
                        }
                        uss.recovery_key_display_focused = false;
                    }
                    _ => {
                        if ch == '\x08' || !ch.is_control() {
                            self.panel_app.user_settings_state.vault_unlock_error = None;
                        }
                    }
                }
            }
        }
    }

    fn text_action_new_profile_name(&mut self, ch: char, action: TextAction) {
        let uss = &mut self.panel_app.user_settings_state;
        match ch {
            '\x09' => {
                // Tab moves to next field in wizard.
                if uss.show_welcome_wizard {
                    self.input_coordinator.borrow_mut().text_fields_mut().blur();
                    let np_wid = WidgetId::from(crate::text_input::NEW_PASSPHRASE);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(np_wid);
                    uss.new_profile_name_focused = false;
                    uss.new_passphrase_focused = true;
                }
            }
            '\x1b' => {
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
                uss.new_profile_name_focused = false;
                uss.show_new_profile_dialog = false;
            }
            _ => {
                if let TextAction::Commit(name) = action {
                    if uss.show_welcome_wizard {
                        self.input_coordinator.borrow_mut().text_fields_mut().blur();
                        let np_wid = WidgetId::from(crate::text_input::NEW_PASSPHRASE);
                        self.input_coordinator.borrow_mut().text_fields_mut().focus(np_wid);
                        uss.new_profile_name_focused = false;
                        uss.new_passphrase_focused = true;
                    } else {
                        uss.new_profile_name_focused = false;
                        let name = name.trim().to_string();
                        if !name.is_empty() {
                            self.pending_updater_cmd = Some(format!("profile_create:{}", name));
                            uss.show_new_profile_dialog = false;
                        }
                    }
                }
            }
        }
    }

    fn text_action_profile_rename(&mut self, action: TextAction) {
        match action {
            TextAction::Commit(new_name) => {
                let new_name = new_name.trim().to_string();
                let uss = &mut self.panel_app.user_settings_state;
                uss.profile_rename_focused = false;
                if !new_name.is_empty() {
                    uss.profile_display_name = new_name.clone();
                    uss.profile_rename_mode = false;
                    uss.profile_rename_target_id = None;
                    self.pending_updater_cmd = Some(format!("profile_rename:{}", new_name));
                }
            }
            _ => {}
        }
    }

    fn text_action_search_overlay(&mut self, action: TextAction) {
        use mlc_core::state::modal_state::OpenModal;
        match action {
            TextAction::Commit(_) => {
                // Enter: select the first result if any, or just close.
                let first_item = match self.modal_state.current {
                    OpenModal::SymbolSearch | OpenModal::CompareSearch => {
                        self.modal_state.symbol_search_results
                            .first()
                            .map(|r| format!("{}:{}:{}", r.symbol, r.exchange_id, r.account_type))
                    }
                    OpenModal::IndicatorSearch => {
                        self.modal_state.hovered_item_id.clone()
                    }
                    _ => None,
                };
                if let Some(item_id) = first_item {
                    let rest = format!("item:{}", item_id);
                    self.handle_search_modal_click(&rest, 0.0, 0.0);
                } else if self.modal_state.current != OpenModal::IndicatorSearch {
                    self.modal_state.close();
                }
            }
            TextAction::TextChanged(ref new_text) => {
                self.modal_state.search_query = new_text.clone();
                if self.modal_state.current == OpenModal::SymbolSearch
                    || self.modal_state.current == OpenModal::CompareSearch
                {
                    let q = self.modal_state.search_query.clone();
                    self.modal_state.symbol_search_results =
                        crate::ChartApp::build_demo_symbol_results(&q, self.core.watchlists().find(self.watchlist_ui.active_list_id), self.core.exchange_symbols().as_map());
                }
            }
            TextAction::Cancel => {
                self.modal_state.close();
            }
            TextAction::None | TextAction::RawInput(_) => {}
        }
    }

    fn text_action_agent_chat(&mut self, ch: char, action: TextAction) {
        let chat_id = WidgetId::from(crate::text_input::AGENT_CHAT);
        if ch == '\r' || ch == '\n' {
            // Enter: send message via gate4agent.
            // The text was committed by on_char('\r') — extract it from the Commit action
            // payload (TextFieldStore may have already cleared the field).
            let text = match &action {
                TextAction::Commit(t) => t.clone(),
                _ => {
                    // Fallback: read from store directly.
                    let coord = self.input_coordinator.borrow();
                    coord.text_fields().text(&chat_id).to_string()
                }
            };
            eprintln!("[gate4agent::chat] Enter via on_char text_len={}", text.len());
            if !text.is_empty() {
                if let Some(leaf_id) = self.sidebar_state.focused_agent_leaf {
                    let desc = self.sidebar_state.agent_leaves.get(&leaf_id).cloned();
                    if let Some(desc) = desc {
                        let _id = desc.instance_id;
                        #[cfg(not(target_arch = "wasm32"))]
                        if let Some(rt) = self.core.native_rt() {
                            match rt.block_on(self.agent.send_chat_instance(_id, &text)) {
                                Ok(()) => {
                                    let buf = self.sidebar_state.agent_input_buffers
                                        .entry(leaf_id).or_default();
                                    buf.clear();
                                    self.input_coordinator.borrow_mut().text_fields_mut().set_text(&chat_id, "");
                                    eprintln!("[gate4agent::chat] Enter via on_char OK");
                                }
                                Err(e) => eprintln!("[gate4agent::chat] Enter via on_char error: {}", e),
                            }
                        }
                    }
                }
            }
        } else {
            // Regular char — TextFieldStore already consumed it via on_char.
            // Sync the leaf's input buffer.
            if let Some(leaf_id) = self.sidebar_state.focused_agent_leaf {
                let new_text = {
                    let coord = self.input_coordinator.borrow();
                    coord.text_fields().text(&chat_id).to_string()
                };
                self.sidebar_state.agent_input_buffers.insert(leaf_id, new_text);
            }
            let _ = action;
        }
    }

    /// Called from dispatch_text_key when AGENT_CHAT is focused (after on_key).
    pub(crate) fn text_action_agent_chat_key(&mut self) {
        let chat_id = WidgetId::from(crate::text_input::AGENT_CHAT);
        if let Some(leaf_id) = self.sidebar_state.focused_agent_leaf {
            let new_text = {
                let coord = self.input_coordinator.borrow();
                coord.text_fields().text(&chat_id).to_string()
            };
            self.sidebar_state.agent_input_buffers.insert(leaf_id, new_text);
        }
    }

    fn text_action_agent_pty(&mut self, action: TextAction) {
        if let TextAction::RawInput(bytes) = action {
            if let Some(leaf_id) = self.sidebar_state.focused_agent_leaf {
                if let Some(desc) = self.sidebar_state.agent_leaves.get(&leaf_id).cloned() {
                    if desc.mode == gate4agent::InstanceMode::Pty {
                        let _text = String::from_utf8_lossy(&bytes).to_string();
                        let _id = desc.instance_id;
                        #[cfg(not(target_arch = "wasm32"))]
                        if let Some(rt) = self.core.native_rt() {
                            let _ = rt.block_on(self.agent.write_pty_instance(_id, &_text));
                        }
                    }
                }
            }
        }
    }

    fn text_action_alert_token(&mut self, ch: char, action: TextAction) {
        // Enter/Escape (char-level): commit text to notification settings.
        if ch == '\r' || ch == '\n' || ch == '\x1b' {
            let tg_wid = WidgetId::from(crate::text_input::ALERT_TG_TOKEN);
            let token = self.input_coordinator.borrow().text_fields().text(&tg_wid).trim().to_string();
            self.panel_app.alert_settings_state.notification_settings.telegram.bot_token = token;
            self.panel_app.alert_settings_state.notification_settings_dirty = true;
            self.input_coordinator.borrow_mut().text_fields_mut().blur();
            return;
        }
        // Commit/Cancel from key events also commit.
        match action {
            TextAction::Commit(_) | TextAction::Cancel => {
                let tg_wid = WidgetId::from(crate::text_input::ALERT_TG_TOKEN);
                let token = self.input_coordinator.borrow().text_fields().text(&tg_wid).trim().to_string();
                self.panel_app.alert_settings_state.notification_settings.telegram.bot_token = token;
                self.panel_app.alert_settings_state.notification_settings_dirty = true;
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
            }
            _ => {}
        }
    }

    fn text_action_bar_mode_param(&mut self, action: TextAction) {
        match action {
            TextAction::Commit(text) => {
                let active_ct = self.panel_app.toolbar_state.active_param_chart_type.clone();
                if active_ct.as_deref() == Some("footprint") {
                    self.process_chart_out_event(
                        mlc_core::events::ChartOutEvent::UpdateChartTypeInterval { interval: text }
                    );
                } else if let Ok(v) = text.trim().parse::<f64>() {
                    self.process_chart_out_event(
                        mlc_core::events::ChartOutEvent::UpdateChartTypeParam { value: v }
                    );
                } else {
                    self.input_coordinator.borrow_mut().text_fields_mut().blur();
                }
                self.panel_app.toolbar_state.active_param_chart_type = None;
            }
            TextAction::Cancel => {
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
                self.panel_app.toolbar_state.active_param_chart_type = None;
            }
            TextAction::TextChanged(_) | TextAction::None | TextAction::RawInput(_) => {}
        }
    }

    /// Commit handler for the `timeframe_selector` dropdown's visible inline
    /// TF editor (user-granularity-templates-inner arc, кусок 2) — SHARED by
    /// the kline `Time`-flavor editor (`AddTf`/`EditTf`) and the footprint
    /// bucket-interval editor (`AddInterval`/`EditInterval`, per-family
    /// scoping arc 2026-07-16), since both mint the SAME `TF_FAVORITE_INPUT`
    /// text field (see `ChartApp::granularity_edit_render`) and accept the
    /// same `Timeframe::parse`-shaped grammar. Which
    /// `GranularityTarget`/profile bucket the commit lands on is decided by
    /// `self.granularity_edit.kind`, not by the field itself. Validates via
    /// `Timeframe::parse`; invalid input does NOT blur — the field stays
    /// focused so the user sees their (rejected) text and can keep typing
    /// (owner spec: "Invalid → do NOT blur, keep editing"). Valid input
    /// dispatches `CommitGranularityCustom`, which itself clears
    /// `granularity_edit` and applies the value.
    fn text_action_tf_favorite_input(&mut self, action: TextAction) {
        match action {
            TextAction::Commit(text) => {
                let name = text.trim().to_string();
                if mlc_core::state::Timeframe::parse(&name).is_none() {
                    return;
                }
                let is_interval_edit = matches!(
                    self.granularity_edit.as_ref().map(|e| &e.kind),
                    Some(crate::GranularityEditKind::AddInterval)
                        | Some(crate::GranularityEditKind::EditInterval { .. })
                );
                let target = if is_interval_edit {
                    mlc_core::chart_type::GranularityTarget::Interval { name }
                } else {
                    mlc_core::chart_type::GranularityTarget::Tf { name }
                };
                self.process_chart_out_event(
                    mlc_core::events::ChartOutEvent::CommitGranularityCustom { target }
                );
            }
            TextAction::Cancel => {
                self.granularity_edit = None;
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
            }
            TextAction::TextChanged(_) | TextAction::None | TextAction::RawInput(_) => {}
        }
    }

    /// Commit handler for the `timeframe_selector` dropdown's visible inline
    /// numeric-param editor (`ResolutionKind::Param` chart types) —
    /// user-granularity-templates-inner arc, кусок 2. Parses a positive
    /// `f64`; invalid input does NOT blur (same contract as
    /// `text_action_tf_favorite_input`). Scoped to the currently active
    /// window's chart_type.
    fn text_action_param_favorite_input(&mut self, action: TextAction) {
        match action {
            TextAction::Commit(text) => {
                let Ok(value) = text.trim().parse::<f64>() else {
                    return;
                };
                if value <= 0.0 {
                    return;
                }
                let Some(chart_type) = self.panel_app.panel_grid.active_container_id()
                    .and_then(|cid| self.chart_type_for_chart(cid))
                else {
                    self.granularity_edit = None;
                    self.input_coordinator.borrow_mut().text_fields_mut().blur();
                    return;
                };
                self.process_chart_out_event(
                    mlc_core::events::ChartOutEvent::CommitGranularityCustom {
                        target: mlc_core::chart_type::GranularityTarget::Param {
                            chart_type: chart_type.to_string(),
                            value,
                        },
                    }
                );
            }
            TextAction::Cancel => {
                self.granularity_edit = None;
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
            }
            TextAction::TextChanged(_) | TextAction::None | TextAction::RawInput(_) => {}
        }
    }

    fn text_action_prim_settings_field(&mut self, action: TextAction) {
        match action {
            TextAction::Commit(_) => {
                self.commit_prim_editing_text();
            }
            TextAction::Cancel => {
                self.panel_app.primitive_settings_state.active_field_id = None;
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
            }
            TextAction::TextChanged(_) | TextAction::None | TextAction::RawInput(_) => {}
        }
    }

    fn text_action_ind_settings_field(&mut self, action: TextAction) {
        match action {
            TextAction::Commit(_) => {
                self.commit_ind_editing_text();
            }
            TextAction::Cancel => {
                self.panel_app.indicator_settings_state.active_field_id = None;
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
            }
            TextAction::TextChanged(_) | TextAction::None | TextAction::RawInput(_) => {}
        }
    }

    fn text_action_chart_settings_field(&mut self, action: TextAction) {
        match action {
            TextAction::Commit(_) => {
                self.commit_chart_settings_editing_text();
            }
            TextAction::Cancel => {
                self.panel_app.chart_settings_state.active_field_id = None;
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
            }
            TextAction::TextChanged(_) | TextAction::None | TextAction::RawInput(_) => {}
        }
    }

    fn text_action_cmp_settings_field(&mut self, action: TextAction) {
        match action {
            TextAction::Commit(_) => {
                self.commit_cmp_editing_text();
            }
            TextAction::Cancel => {
                self.panel_app.compare_settings_state.active_field_id = None;
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
            }
            TextAction::TextChanged(_) | TextAction::None | TextAction::RawInput(_) => {}
        }
    }

    fn text_action_template_name(&mut self, id: &str, action: TextAction) {
        use crate::text_input::*;
        match action {
            TextAction::Commit(_) => {
                if id == PRIMITIVE_SETTINGS_TEMPLATE_NAME {
                    self.handle_prim_tmpl_modal_click("save");
                } else if id == INDICATOR_SETTINGS_TEMPLATE_NAME {
                    self.handle_ind_tmpl_modal_click("save");
                } else if id == COMPARE_SETTINGS_TEMPLATE_NAME {
                    self.handle_cmp_tmpl_modal_click("save");
                } else if id == CHART_SETTINGS_TEMPLATE_NAME {
                    self.handle_chart_tmpl_modal_click("save");
                }
            }
            TextAction::Cancel => {
                if id == PRIMITIVE_SETTINGS_TEMPLATE_NAME {
                    self.handle_prim_tmpl_modal_click("cancel");
                } else if id == INDICATOR_SETTINGS_TEMPLATE_NAME {
                    self.handle_ind_tmpl_modal_click("cancel");
                } else if id == COMPARE_SETTINGS_TEMPLATE_NAME {
                    self.handle_cmp_tmpl_modal_click("cancel");
                } else if id == CHART_SETTINGS_TEMPLATE_NAME {
                    self.handle_chart_tmpl_modal_click("cancel");
                }
            }
            TextAction::TextChanged(_) | TextAction::None | TextAction::RawInput(_) => {}
        }
    }
}
