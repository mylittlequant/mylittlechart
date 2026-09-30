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
    /// Covers (in order): ALERT_TG_TOKEN, REDEEM_CODE, FORUM_TITLE,
    /// FORUM_BODY, FORUM_REPLY, CRED_WEBHOOK_URL,
    /// CRED_ADD_EXCHANGE, CRED_ADD_LABEL, CRED_ADD_API_KEY,
    /// CRED_ADD_API_SECRET, CRED_ADD_PASSPHRASE, PROFILE_RENAME,
    /// ACCOUNT_NICK, NEW_PASSPHRASE, NEW_PROFILE_NAME, RECOVERY_KEY,
    /// RECOVERY_KEY_DISPLAY, CONFIRM_PASSPHRASE, BAR_MODE_PARAM,
    /// TF_FAVORITE_INPUT, PARAM_FAVORITE_INPUT, HEX_COLOR, AGENT_PTY,
    /// AGENT_CHAT.
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

        if self.blur_if_stale_focus(&focused_id) {
            return false;
        }

        // Only route early fields; late fields (chart_browser, presets, settings, search)
        // are handled after global undo/redo by dispatch_text_key_late.
        #[allow(unused_mut)]
        let mut is_early_field = matches!(
            focused_id.as_str(),
            ALERT_TG_TOKEN
                | CRED_WEBHOOK_URL
                | CRED_ADD_EXCHANGE
                | CRED_ADD_LABEL
                | CRED_ADD_API_KEY
                | CRED_ADD_API_SECRET
                | CRED_ADD_PASSPHRASE
                | PROFILE_RENAME
                | ACCOUNT_NICK
                | REDEEM_CODE
                | CHECKOUT_KEY_INPUT
                | NEW_PASSPHRASE
                | NEW_PROFILE_NAME
                | RECOVERY_KEY
                | RECOVERY_KEY_DISPLAY
                | CONFIRM_PASSPHRASE
                | BAR_MODE_PARAM
                | TF_FAVORITE_INPUT
                | PARAM_FAVORITE_INPUT
                | HEX_COLOR
                | AGENT_PTY
                | AGENT_CHAT
        );
        // `REDEEM_CODE` moved out of this cfg block 2026-09-23: the row it
        // drives is the Settings → Account tab now (`ACCOUNT_NICK`'s own
        // neighbor above), not the cloud-only login overlay — see
        // `RedeemStatus`'s own doc. The three Forum fields and the two DM
        // fields (paid-only tab) are still genuinely cloud-only.
        #[cfg(feature = "cloud")]
        {
            is_early_field |= matches!(
                focused_id.as_str(),
                FORUM_TITLE | FORUM_BODY | FORUM_REPLY | FORUM_SEARCH | DM_DRAFT | DM_PEER
                    | SHARE_SETTINGS_NICK
                    | FRIENDS_NICK
            );
        }
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
    /// PRESET_NAME_INPUT, template name fields, settings sub-fields, search overlay,
    /// the inline editors (PRIM_INLINE_TEXT, CHROME_TAB_RENAME),
    /// and the OrderEntry/RiskCalculator per-instance `oe_field`/`rc_field` ids.
    pub(crate) fn dispatch_text_key_late(&mut self, key: &KeyPress) -> bool {
        use crate::text_input::*;

        // Identify focused field.
        let focused_id = {
            let ic = self.input_coordinator.borrow();
            ic.text_fields().focused().cloned()
        };
        let Some(focused_id) = focused_id else { return false };

        if self.blur_if_stale_focus(&focused_id) {
            return false;
        }

        // OrderEntry/RiskCalculator fields are per-instance dynamic ids
        // (`"{slot_prefix}:oe_field"` / `"{slot_prefix}:rc_field"`, minted by
        // `mlc-app-panels`' `render()` off the same `slot_prefix` the
        // sidebar assigns each panel instance) — not fixed consts like every
        // other entry here, so they need a suffix-match arm rather than a
        // `matches!` literal. First (and, as of this migration, only)
        // prefix/suffix-match precedent in this dispatcher.
        let is_dynamic_trading_field = focused_id.as_str().ends_with(":oe_field")
            || focused_id.as_str().ends_with(":rc_field")
            || focused_id.as_str().starts_with(DOM_DEPTH_PCT_PREFIX);

        let is_late_field = is_dynamic_trading_field || matches!(
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
                | PRIM_INLINE_TEXT
                | CHROME_TAB_RENAME
                | SYMBOL_SEARCH
                | INDICATOR_SEARCH
                | COMPARE_SEARCH
                | SYMBOL_QUICK_SEARCH
                | KEY_LABEL_INPUT
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
        // Per-panel field: the id carries the panel, so it is matched by
        // prefix before the table of fixed ids.
        if let Some(rest) = id.as_str().strip_prefix(DOM_DEPTH_PCT_PREFIX) {
            if let Ok(raw) = rest.parse::<u64>() {
                let pid = mlc_core::state::PanelId(raw);
                match action {
                    TextAction::Commit(text) => self.commit_dom_depth_pct(pid, &text),
                    TextAction::Cancel => {
                        self.input_coordinator.borrow_mut().text_fields_mut().blur();
                    }
                    _ => {}
                }
            }
            return;
        }
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
            ACCOUNT_NICK                     => self.text_action_account_nick(ch, action),
            REDEEM_CODE                      => self.text_action_redeem_code(ch, action),
            CHECKOUT_KEY_INPUT               => self.text_action_checkout_key(ch, action),
            SYMBOL_SEARCH | INDICATOR_SEARCH | COMPARE_SEARCH
                                             => self.text_action_search_overlay(action),
            AGENT_CHAT                       => self.text_action_agent_chat(ch, action),
            AGENT_PTY                        => self.text_action_agent_pty(action),
            #[cfg(feature = "cloud")]
            FORUM_TITLE                      => self.text_action_forum_title(ch, action),
            #[cfg(feature = "cloud")]
            FORUM_BODY                        => self.text_action_forum_body(ch, action),
            #[cfg(feature = "cloud")]
            FORUM_REPLY                       => self.text_action_forum_reply(ch, action),
            #[cfg(feature = "cloud")]
            FORUM_SEARCH                      => self.text_action_forum_search(ch, action),
            #[cfg(feature = "cloud")]
            DM_DRAFT                          => self.text_action_dm_draft(ch, action),
            #[cfg(feature = "cloud")]
            DM_PEER                           => self.text_action_dm_peer(ch, action),
            #[cfg(feature = "cloud")]
            DM_CHAT_FILTER                    => self.text_action_dm_chat_filter(ch, action),
            #[cfg(feature = "cloud")]
            SHARE_SETTINGS_NICK               => self.text_action_share_settings_nick(ch, action),
            FRIENDS_NICK                      => self.text_action_friends_nick(ch, action),
            ALERT_TG_TOKEN                   => self.text_action_alert_token(ch, action),
            CRED_WEBHOOK_URL                 => self.text_action_cred_webhook_url(ch, action),
            CRED_ADD_EXCHANGE                => self.text_action_cred_add_field(crate::input::credentials::CredAddField::Exchange, ch, action),
            CRED_ADD_LABEL                   => self.text_action_cred_add_field(crate::input::credentials::CredAddField::Label, ch, action),
            CRED_ADD_API_KEY                 => self.text_action_cred_add_field(crate::input::credentials::CredAddField::ApiKey, ch, action),
            CRED_ADD_API_SECRET              => self.text_action_cred_add_field(crate::input::credentials::CredAddField::ApiSecret, ch, action),
            CRED_ADD_PASSPHRASE              => self.text_action_cred_add_field(crate::input::credentials::CredAddField::Passphrase, ch, action),
            BAR_MODE_PARAM                   => self.text_action_bar_mode_param(action),
            WATERMARK_TEXT                   => self.text_action_watermark_text(action),
            TF_FAVORITE_INPUT                => self.text_action_tf_favorite_input(action),
            PARAM_FAVORITE_INPUT             => self.text_action_param_favorite_input(action),
            SYMBOL_QUICK_SEARCH              => self.text_action_symbol_quick_search(action),
            KEY_LABEL_INPUT                  => self.text_action_key_label(action),
            PRIMITIVE_SETTINGS_FIELD         => self.text_action_prim_settings_field(action),
            INDICATOR_SETTINGS_FIELD         => self.text_action_ind_settings_field(action),
            CHART_SETTINGS_FIELD             => self.text_action_chart_settings_field(action),
            COMPARE_SETTINGS_FIELD           => self.text_action_cmp_settings_field(action),
            PRIM_INLINE_TEXT                 => self.text_action_prim_inline_text(action),
            PRIMITIVE_SETTINGS_TEMPLATE_NAME
            | INDICATOR_SETTINGS_TEMPLATE_NAME
            | CHART_SETTINGS_TEMPLATE_NAME
            | COMPARE_SETTINGS_TEMPLATE_NAME => self.text_action_template_name(id.as_str(), action),
            s if s.ends_with(":oe_field") => self.text_action_oe_field(id, action),
            s if s.ends_with(":rc_field") => self.text_action_rc_field(id, action),
            _ => {}
        }
    }

    /// Enter/Escape handler for OrderEntry's per-instance shared numeric
    /// field (`"{slot_prefix}:oe_field"`). Commit reuses
    /// `panel_click::commit_oe_field` (the exact same helper the app-layer
    /// click router uses for auto-commit-on-field-switch and
    /// blur-before-submit); Cancel just clears `active_field` and blurs
    /// (`TextFieldStore::on_char('\x1b')` already reverted the store's own
    /// text to `original_text`, but the panel never reads it after Cancel).
    fn text_action_oe_field(&mut self, id: &WidgetId, action: TextAction) {
        let slot_prefix = id.as_str().strip_suffix(":oe_field").unwrap_or_default();
        let Some(target) = self.oe_target_for_slot_prefix(slot_prefix) else { return };
        match action {
            TextAction::Commit(_) => self.commit_oe_field(target, id),
            TextAction::Cancel => {
                if let Some(state) = self.order_entry_mut(target) {
                    state.active_field = None;
                }
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
            }
            TextAction::TextChanged(_) | TextAction::None | TextAction::RawInput(_) => {}
        }
    }

    /// RiskCalculator analogue of `text_action_oe_field`.
    fn text_action_rc_field(&mut self, id: &WidgetId, action: TextAction) {
        let slot_prefix = id.as_str().strip_suffix(":rc_field").unwrap_or_default();
        let Some(target) = self.rc_target_for_slot_prefix(slot_prefix) else { return };
        match action {
            TextAction::Commit(_) => self.commit_rc_field(target, id),
            TextAction::Cancel => {
                if let Some(state) = self.risk_calculator_mut(target) {
                    state.active_field = None;
                }
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
            }
            TextAction::TextChanged(_) | TextAction::None | TextAction::RawInput(_) => {}
        }
    }

    /// Enter/Escape handler for the inline on-chart primitive text editor
    /// (`PRIM_INLINE_TEXT`, inline-prim-text arc 2026-08-26). Commit writes
    /// through `commit_inline_prim_text_edit` (undo-recorded); Cancel is
    /// the in-band `\x1b` char path — the native shell routes Escape to
    /// `on_escape()` instead, which cancels the edit directly. Either way
    /// Cancel clears state + blurs and NEVER commits.
    fn text_action_prim_inline_text(&mut self, action: TextAction) {
        match action {
            TextAction::Commit(_) => self.commit_inline_prim_text_edit(),
            TextAction::Cancel => self.cancel_inline_prim_text_edit(),
            TextAction::TextChanged(_) | TextAction::None | TextAction::RawInput(_) => {}
        }
    }

    /// Stale-focus guard shared by [`Self::dispatch_text_key_early`] and
    /// [`Self::dispatch_text_key_late`].
    ///
    /// Both callers are about to consume the current key purely because
    /// `focused_id` is focused in the shared `TextFieldStore` — even a key
    /// with no mapping for that field (`dispatch_text_key_early` consumes
    /// it anyway, see its own doc). If some close path along the 15+
    /// independent modal/inline-editor closes in this crate reset its
    /// `is_open`/`show_*` flag but forgot the matching
    /// `text_fields_mut().blur()`, `focused_id` keeps pointing at a field
    /// whose owner has stopped drawing — and every hotkey in the app dies
    /// silently for the rest of the session, since both dispatchers run
    /// before every other hotkey arm in `on_key_press`.
    ///
    /// Detected via `TextFieldStore::focused_is_stale` — the field's own
    /// per-frame liveness stamp (`update_field` or `mark_seen`, refreshed
    /// by its host every frame it actually draws) has gone quiet. Self-heals:
    /// blurs the field, logs the id so a live session can still name the
    /// exact leaking close path, and returns `true` so the caller lets the
    /// key fall through to normal hotkey routing instead of swallowing it.
    ///
    /// Regression tripwire (debug builds only): every focusable field's
    /// draw site is expected to call `update_field`/`mark_seen` on every
    /// frame it draws that field — `focused_is_stale` cannot protect a
    /// field that never got either call even once (it self-heals only
    /// after `FOCUS_STALE_FRAME_LAG` frames of silently eaten keystrokes).
    /// `TextFieldStore::focused_was_never_stamped` catches that shape
    /// immediately instead of waiting out the lag, so a NEW field added
    /// later without its stamp wired in is caught here in development, not
    /// reported by the owner.
    fn blur_if_stale_focus(&mut self, focused_id: &WidgetId) -> bool {
        #[cfg(debug_assertions)]
        {
            // A warning, not a debug_assert: the owner's wasm stand is a
            // `--dev` build, where a panic here would take the whole app
            // down over a missing stamp — and a field focused in the same
            // frame a key arrives, before its first draw, is legitimately
            // unstamped for that one frame.
            let never_stamped = self.input_coordinator.borrow().text_fields().focused_was_never_stamped();
            if never_stamped {
                log::warn!(
                    "[stale-focus] text field '{}' is focused but has NEVER been stamped via \
                     update_field()/mark_seen() — its draw site is missing the stamp call that \
                     TextFieldStore::focused_is_stale() needs to self-heal a leaked focus. \
                     Add the missing stamp at wherever this field is drawn.",
                    focused_id.as_str()
                );
            }
        }
        if !self.input_coordinator.borrow().text_fields().focused_is_stale() {
            return false;
        }
        log::warn!(
            "[stale-focus] blurring text field '{}': it stopped being drawn while still focused \
             (a modal/editor close path likely forgot text_fields_mut().blur())",
            focused_id.as_str()
        );
        self.input_coordinator.borrow_mut().text_fields_mut().blur();
        true
    }

    /// While the profile-manager modal is shown, every hotkey except its
    /// own passphrase/recovery/name fields must stay dead — otherwise
    /// Escape, arrows, etc. would operate on the hidden chart UI behind
    /// it. But `show_profile_manager` is reset by 15+ independent close
    /// paths across both shells, and a path that forgets the reset would
    /// otherwise leave this gate swallowing every hotkey forever.
    ///
    /// Self-heals: if the flag is set but keyboard focus sits on a field
    /// this modal does not own, that is proof the person is interacting
    /// with something else the profile manager cannot be drawing over —
    /// so the flag is corrected here instead of trusted. Pages with no
    /// text field of their own (`ProfileList`, `CreateNew`'s list view)
    /// leave nothing focused; an empty focus is not evidence the modal is
    /// gone, so it is not treated as stale.
    pub(crate) fn profile_manager_blocks_text_input(&mut self) -> bool {
        let uss = &self.panel_app.user_settings_state;
        if !uss.show_profile_manager {
            return false;
        }
        if uss.new_passphrase_focused
            || uss.new_profile_name_focused
            || uss.recovery_key_display_focused
            || uss.confirm_passphrase_focused
        {
            return false;
        }
        use crate::text_input::{CONFIRM_PASSPHRASE, NEW_PASSPHRASE, NEW_PROFILE_NAME, RECOVERY_KEY_DISPLAY};
        let focused_elsewhere = {
            let ic = self.input_coordinator.borrow();
            match ic.text_fields().focused().map(|id| id.as_str()) {
                None => false,
                Some(NEW_PASSPHRASE | CONFIRM_PASSPHRASE | NEW_PROFILE_NAME | RECOVERY_KEY_DISPLAY) => false,
                Some(_) => true,
            }
        };
        if focused_elsewhere {
            log::warn!(
                "[stale-focus] show_profile_manager was still true with keyboard focus on an \
                 unrelated field; clearing it (owning close path likely missed the reset)"
            );
            self.panel_app.user_settings_state.show_profile_manager = false;
            return false;
        }
        true
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
                            log::debug!("[WatchlistGroupName] created new list '{}' id={}", name, new_id);
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
                                log::debug!("[WatchlistGroupName] renamed list id={} to '{}'", id, name);
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

    /// Server tab's agent-API key manager "label" field (agent-api-auth-
    /// and-secret-store plan, piece 4). Enter submits the same "Create key"
    /// action the button click does — see
    /// [`ChartApp::submit_key_create_request`] (`panel_click.rs`) for the
    /// shared body.
    fn text_action_key_label(&mut self, action: TextAction) {
        match action {
            TextAction::Commit(_) => self.submit_key_create_request(),
            TextAction::Cancel => {
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
                self.panel_app.user_settings_state.key_label_focused = false;
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
                            log::debug!("[ChartApp] profile_mgr: set_new_passphrase submitted via Enter");
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
                            log::debug!("[ChartApp] profile_mgr: recovery unlock submitted via Enter");
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

    /// The single-profile row's account-nickname field
    /// (`UserSettingsState::nick_edit_mode`).
    ///
    /// Enter validates locally via
    /// [`mlc_app_layout::ui::modal_settings::validate_nick`] and, if it
    /// passes, arms the same one-shot `nick_save_requested` the Save button
    /// sets (so key and click cannot disagree) and marks the status
    /// `Working` immediately — the shell owns the actual round trip, same
    /// division as `text_action_redeem_code`. Escape closes the edit and
    /// discards whatever was typed: unlike `REDEEM_CODE` (an always-visible
    /// box, so Escape there only blurs), this field only exists while
    /// `nick_edit_mode` is true, so there is a mode to leave.
    fn text_action_account_nick(&mut self, ch: char, action: TextAction) {
        let submit = |chart: &mut ChartApp| {
            let wid = WidgetId::from(crate::text_input::ACCOUNT_NICK);
            let nick = chart.input_coordinator.borrow().text_fields().text(&wid).trim().to_string();
            match mlc_app_layout::ui::modal_settings::validate_nick(&nick) {
                Ok(()) => {
                    chart.panel_app.user_settings_state.nick_status =
                        mlc_app_layout::ui::modal_settings::NickStatus::Working;
                    chart.panel_app.user_settings_state.nick_save_requested = true;
                }
                Err(message) => {
                    chart.panel_app.user_settings_state.nick_status =
                        mlc_app_layout::ui::modal_settings::NickStatus::Failed { message };
                }
            }
        };
        let cancel = |chart: &mut ChartApp| {
            let uss = &mut chart.panel_app.user_settings_state;
            uss.nick_edit_mode = false;
            uss.nick_edit_focused = false;
            uss.nick_status = mlc_app_layout::ui::modal_settings::NickStatus::Idle;
            let wid = WidgetId::from(crate::text_input::ACCOUNT_NICK);
            chart.input_coordinator.borrow_mut().text_fields_mut().clear(&wid);
            chart.input_coordinator.borrow_mut().text_fields_mut().blur();
        };
        if ch == '\r' || ch == '\n' {
            submit(self);
            return;
        }
        if ch == '\x1b' {
            cancel(self);
            return;
        }
        match action {
            TextAction::Commit(_) => submit(self),
            TextAction::Cancel => cancel(self),
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
            log::debug!("[gate4agent::chat] Enter via on_char text_len={}", text.len());
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
                                    log::debug!("[gate4agent::chat] Enter via on_char OK");
                                }
                                Err(e) => log::warn!("[gate4agent::chat] Enter via on_char error: {}", e),
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
        // The token field cannot be focused while the vault is locked (the
        // render layer never registers its hit zone then), so this is
        // defense in depth against a stray focus/key event reaching here.
        if !self.panel_app.alert_settings_state.secrets_available {
            self.input_coordinator.borrow_mut().text_fields_mut().blur();
            return;
        }
        // Enter/Escape (char-level): commit text to notification secrets.
        if ch == '\r' || ch == '\n' || ch == '\x1b' {
            let tg_wid = WidgetId::from(crate::text_input::ALERT_TG_TOKEN);
            let token = self.input_coordinator.borrow().text_fields().text(&tg_wid).trim().to_string();
            self.panel_app.alert_settings_state.notification_secrets.telegram.bot_token = token;
            self.panel_app.alert_settings_state.notification_settings_dirty = true;
            self.input_coordinator.borrow_mut().text_fields_mut().blur();
            return;
        }
        // Commit/Cancel from key events also commit.
        match action {
            TextAction::Commit(_) | TextAction::Cancel => {
                let tg_wid = WidgetId::from(crate::text_input::ALERT_TG_TOKEN);
                let token = self.input_coordinator.borrow().text_fields().text(&tg_wid).trim().to_string();
                self.panel_app.alert_settings_state.notification_secrets.telegram.bot_token = token;
                self.panel_app.alert_settings_state.notification_settings_dirty = true;
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
            }
            _ => {}
        }
    }

    /// The Account tab's gift/trial code field (owner ruling 2026-09-23:
    /// moved here from the old cloud-login overlay's Account view, which
    /// nothing ever opened for an already-signed-in caller — see
    /// `RedeemStatus`'s own doc). NOT `cfg(feature = "cloud")`-gated, same
    /// reasoning `text_action_account_nick` already gives for its own field:
    /// the row it drives compiles in every build.
    ///
    /// Enter submits — the same one-shot the "Redeem" button sets, so the two
    /// routes cannot behave differently. Escape just leaves the field; it does
    /// not close the modal, because a code half-typed into a tab that also
    /// holds Sign out is not something to discard on a stray key.
    ///
    /// The text is deliberately left in place on submit. The shell reads it
    /// when it drains the flag, and if the code is refused the person can fix
    /// one character rather than retype ten.
    fn text_action_redeem_code(&mut self, ch: char, action: TextAction) {
        let submit = |chart: &mut ChartApp| {
            let wid = WidgetId::from(crate::text_input::REDEEM_CODE);
            let code = chart.input_coordinator.borrow().text_fields().text(&wid).trim().to_string();
            if code.is_empty() {
                return;
            }
            chart.panel_app.user_settings_state.redeem_requested = true;
        };
        if ch == '\r' || ch == '\n' {
            submit(self);
            return;
        }
        if ch == '\x1b' {
            self.input_coordinator.borrow_mut().text_fields_mut().blur();
            return;
        }
        match action {
            TextAction::Commit(_) => submit(self),
            TextAction::Cancel => {
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
            }
            _ => {}
        }
    }

    /// The checkout modal's account-key box. Enter signs in; Escape leaves the
    /// modal (the box is cleared by the checkout's own per-frame release).
    fn text_action_checkout_key(&mut self, ch: char, action: TextAction) {
        if ch == '\r' || ch == '\n' {
            self.checkout_submit_key();
            return;
        }
        if ch == '\x1b' {
            self.checkout_dismiss();
            return;
        }
        match action {
            TextAction::Commit(_) => self.checkout_submit_key(),
            TextAction::Cancel => self.checkout_dismiss(),
            _ => {}
        }
    }

    /// Abandon the Forum-v2 compose form: back to the board list, clear
    /// both fields, drop focus. Used by `FORUM_TITLE`/`FORUM_BODY`'s own
    /// Escape handling below. `panel_click.rs`'s `"forum2:compose_cancel"`
    /// arm inlines the same steps rather than calling this — it already
    /// holds a named `&mut` borrow of `community_state`
    /// (`let comm = &mut self.panel_app.community_state;`), and a method
    /// call taking `&mut self` cannot run alongside that borrow. Keep the
    /// two in sync by hand if this logic ever changes.
    ///
    /// `FORUM_TITLE`/`FORUM_BODY` are the same `TextFieldStore` ids Forum-v2's
    /// compose view reuses from v1 (`forum_v2_overlay.rs`'s `draw_compose`) —
    /// v1's own `ForumSubView`/`ForumRequest` no longer render anywhere
    /// (the Community modal's own Forum tab draws `ForumV2State`, see that
    /// type's own doc), so this handler (and its two siblings below) target
    /// `forum2` instead.
    #[cfg(feature = "cloud")]
    fn cancel_forum_compose(&mut self) {
        self.panel_app.community_state.forum2.reset_to_boards();
        let mut tf = self.input_coordinator.borrow_mut();
        let tf = tf.text_fields_mut();
        tf.clear(&WidgetId::from(crate::text_input::FORUM_TITLE));
        tf.clear(&WidgetId::from(crate::text_input::FORUM_BODY));
        tf.blur();
    }

    /// Forum-v2 compose view's title field. Enter validates locally
    /// (`validate_forum_title`) and, if it passes, moves focus to
    /// `FORUM_BODY` — this field has no natural "next" key otherwise, so
    /// Enter takes that role instead of submitting early on a form that
    /// still needs a body. Escape abandons the whole compose form (see
    /// `cancel_forum_compose`), not just this field — a title half-typed
    /// into a form with nothing worth keeping is not something to leave
    /// half-open.
    #[cfg(feature = "cloud")]
    fn text_action_forum_title(&mut self, ch: char, action: TextAction) {
        let advance = |chart: &mut ChartApp| {
            let wid = WidgetId::from(crate::text_input::FORUM_TITLE);
            let title = chart.input_coordinator.borrow().text_fields().text(&wid).trim().to_string();
            match mlc_app_layout::ui::modal_settings::validate_forum_title(&title) {
                Ok(()) => {
                    let body_wid = WidgetId::from(crate::text_input::FORUM_BODY);
                    chart.input_coordinator.borrow_mut().text_fields_mut().focus(body_wid);
                    chart.panel_app.community_state.forum2.last_error = None;
                }
                Err(message) => {
                    chart.panel_app.community_state.forum2.last_error = Some(message);
                }
            }
        };
        if ch == '\r' || ch == '\n' {
            advance(self);
            return;
        }
        if ch == '\x1b' {
            self.cancel_forum_compose();
            return;
        }
        match action {
            TextAction::Commit(_) => advance(self),
            TextAction::Cancel => self.cancel_forum_compose(),
            _ => {}
        }
    }

    /// Forum-v2 compose view's body field — the form's last field, so Enter
    /// here submits the whole thread (both fields' current text, re-read
    /// from the store the same way `text_action_account_nick`'s `submit`
    /// does). Escape abandons the compose form, same as `FORUM_TITLE`'s own
    /// Escape. Mirrors `panel_click.rs`'s `"forum2:compose_post"` click arm
    /// exactly, so Enter-from-body and clicking Post cannot disagree.
    #[cfg(feature = "cloud")]
    fn text_action_forum_body(&mut self, ch: char, action: TextAction) {
        let submit = |chart: &mut ChartApp| {
            let title_wid = WidgetId::from(crate::text_input::FORUM_TITLE);
            let body_wid = WidgetId::from(crate::text_input::FORUM_BODY);
            let (title, body) = {
                let coord = chart.input_coordinator.borrow();
                let tf = coord.text_fields();
                (tf.text(&title_wid).trim().to_string(), tf.text(&body_wid).trim().to_string())
            };
            let slug = match &chart.panel_app.community_state.forum2.view {
                mlc_app_layout::ui::modal_settings::ForumV2View::Compose { slug } => slug.clone(),
                _ => return,
            };
            let validated = mlc_app_layout::ui::modal_settings::validate_forum_title(&title)
                .and_then(|()| mlc_app_layout::ui::modal_settings::validate_forum_body(&body));
            match validated {
                Ok(()) => {
                    let f2 = &mut chart.panel_app.community_state.forum2;
                    f2.last_error = None;
                    f2.create_thread_pending = true;
                    f2.request = Some(mlc_app_layout::ui::modal_settings::ForumV2Request::CreateThread {
                        slug, title, body,
                    });
                }
                Err(message) => {
                    chart.panel_app.community_state.forum2.last_error = Some(message);
                }
            }
        };
        if ch == '\r' || ch == '\n' {
            submit(self);
            return;
        }
        if ch == '\x1b' {
            self.cancel_forum_compose();
            return;
        }
        match action {
            TextAction::Commit(_) => submit(self),
            TextAction::Cancel => self.cancel_forum_compose(),
            _ => {}
        }
    }

    /// Forum-v2 thread view's reply field. Enter validates locally
    /// (`validate_forum_body`) and, if it passes, arms
    /// `ForumV2Request::Reply` (nested under `reply_to` when a specific
    /// reply's own "Reply" row was picked first) and marks
    /// `ForumV2State::reply_pending` — same submit-on-Enter idiom
    /// `text_action_redeem_code` uses, and the same pending-flag idiom v1's
    /// own `forum_reply_pending` uses (see that field's doc for why one is
    /// needed). Escape clears the field and drops focus WITHOUT leaving the
    /// thread (unlike `FORUM_TITLE`/`FORUM_BODY`'s Escape, which abandons
    /// the whole compose form) — there is no "form" here to abandon, just a
    /// reply someone changed their mind about typing. Mirrors
    /// `panel_click.rs`'s `"forum2:reply_submit"` click arm exactly.
    #[cfg(feature = "cloud")]
    fn text_action_forum_reply(&mut self, ch: char, action: TextAction) {
        let submit = |chart: &mut ChartApp| {
            let wid = WidgetId::from(crate::text_input::FORUM_REPLY);
            let body = chart.input_coordinator.borrow().text_fields().text(&wid).trim().to_string();
            let (thread_id, parent_reply_id) = match &chart.panel_app.community_state.forum2.view {
                mlc_app_layout::ui::modal_settings::ForumV2View::Thread { id, reply_to, .. } => (*id, *reply_to),
                _ => return,
            };
            match mlc_app_layout::ui::modal_settings::validate_forum_body(&body) {
                Ok(()) => {
                    let f2 = &mut chart.panel_app.community_state.forum2;
                    f2.last_error = None;
                    f2.reply_pending = true;
                    f2.request = Some(mlc_app_layout::ui::modal_settings::ForumV2Request::Reply {
                        thread_id, body, parent_reply_id,
                    });
                }
                Err(message) => {
                    chart.panel_app.community_state.forum2.last_error = Some(message);
                }
            }
        };
        let cancel = |chart: &mut ChartApp| {
            let wid = WidgetId::from(crate::text_input::FORUM_REPLY);
            chart.input_coordinator.borrow_mut().text_fields_mut().clear(&wid);
            chart.input_coordinator.borrow_mut().text_fields_mut().blur();
        };
        if ch == '\r' || ch == '\n' {
            submit(self);
            return;
        }
        if ch == '\x1b' {
            cancel(self);
            return;
        }
        match action {
            TextAction::Commit(_) => submit(self),
            TextAction::Cancel => cancel(self),
            _ => {}
        }
    }

    /// The Community modal's compact inline search field (owner review
    /// 2026-09-09, community-modal-and-forum-ladder arc, piece 2 item d —
    /// replaces the old full-page Boards-view search box this doc used to
    /// describe). Enter fires `ForumV2Request::Search` immediately (the
    /// same request the ~300ms debounce in the shell driver's per-frame
    /// poll would otherwise fire); every other keystroke just (re)arms that
    /// debounce (`ForumV2State::arm_quick_search_debounce`) — nothing
    /// fetches on this path. The answer lands in `ForumV2State::
    /// quick_search_results` (the inline dropdown), never `view` — the
    /// full-page `ForumV2View::Search` is reached only via the dropdown's
    /// own "More results" row (`panel_click.rs`'s `"forum2:quick_more"`
    /// arm). Escape collapses the box outright (`close_quick_search`)
    /// instead of just blurring the field.
    #[cfg(feature = "cloud")]
    fn text_action_forum_search(&mut self, ch: char, action: TextAction) {
        let fire = |chart: &mut ChartApp| {
            let wid = WidgetId::from(crate::text_input::FORUM_SEARCH);
            let q = chart.input_coordinator.borrow().text_fields().text(&wid).trim().to_string();
            let f2 = &mut chart.panel_app.community_state.forum2;
            f2.quick_search_due_at = None;
            if q.is_empty() {
                f2.quick_search_query = None;
                f2.quick_search_results = Vec::new();
                return;
            }
            f2.last_error = None;
            f2.quick_search_pending = true;
            f2.request = Some(mlc_app_layout::ui::modal_settings::ForumV2Request::Search { q, board: None });
        };
        if ch == '\r' || ch == '\n' {
            fire(self);
            return;
        }
        if ch == '\x1b' {
            self.panel_app.community_state.forum2.close_quick_search();
            self.input_coordinator.borrow_mut().text_fields_mut().blur();
            return;
        }
        match action {
            TextAction::Commit(_) => fire(self),
            TextAction::Cancel => {
                self.panel_app.community_state.forum2.close_quick_search();
            }
            _ => {
                if self.panel_app.community_state.forum2.quick_search_open {
                    self.panel_app.community_state.forum2.arm_quick_search_debounce();
                }
            }
        }
    }

    /// DM Conversation view's draft box. Enter validates non-empty and, if
    /// it passes, arms `DmRequest::Send` and marks `DmState::send_pending`
    /// — same pending-flag idiom as `ForumV2State::reply_pending`/v1's
    /// `forum_reply_pending` (see the latter's doc for why one is needed).
    /// Escape clears the field and drops focus, same as `FORUM_REPLY`'s own
    /// Escape. Mirrors `panel_click.rs`'s `"dm:send"` click arm exactly.
    #[cfg(feature = "cloud")]
    fn text_action_dm_draft(&mut self, ch: char, action: TextAction) {
        let submit = |chart: &mut ChartApp| {
            let wid = WidgetId::from(crate::text_input::DM_DRAFT);
            let text = chart.input_coordinator.borrow().text_fields().text(&wid).trim().to_string();
            if text.is_empty() {
                return;
            }
            let id = match &chart.panel_app.community_state.dm.view {
                mlc_app_layout::ui::modal_settings::DmView::Conversation { id, .. } => *id,
                _ => return,
            };
            let dm = &mut chart.panel_app.community_state.dm;
            dm.last_error = None;
            dm.send_pending = true;
            dm.request = Some(mlc_app_layout::ui::modal_settings::DmRequest::Send { id, plaintext: text });
        };
        let cancel = |chart: &mut ChartApp| {
            let wid = WidgetId::from(crate::text_input::DM_DRAFT);
            chart.input_coordinator.borrow_mut().text_fields_mut().clear(&wid);
            chart.input_coordinator.borrow_mut().text_fields_mut().blur();
        };
        if ch == '\r' || ch == '\n' {
            submit(self);
            return;
        }
        if ch == '\x1b' {
            cancel(self);
            return;
        }
        match action {
            TextAction::Commit(_) => submit(self),
            TextAction::Cancel => cancel(self),
            _ => {}
        }
    }

    /// DM List view's "start a conversation" recipient-search field (owner
    /// ruling 2026-09-09, community-modal-and-forum-ladder arc, piece 2
    /// item b — see `mlc_app_layout::layout::modals::dm_overlay`'s
    /// `draw_open_peer_row`/`draw_lookup_dropdown`). Every keystroke (re)arms
    /// the debounced `DmRequest::Lookup` fetch
    /// (`DmState::arm_lookup_debounce`), mirroring `text_action_forum_
    /// search`'s own `ForumV2State::arm_quick_search_debounce`. Enter opens
    /// the highlighted (top) dropdown result directly when one exists;
    /// otherwise it falls back to the typed text as an exact label — the
    /// field's original behaviour, kept so a public id typed in full still
    /// works even when the fuzzy lookup finds nothing matching it. Escape
    /// closes the dropdown outright (`close_lookup_dropdown`), same
    /// "collapse instead of just blur" rule `text_action_forum_search`
    /// gives its own inline box. Mirrors `panel_click.rs`'s
    /// `"dm:lookup_result:{i}"` click arm for the row-click path.
    #[cfg(feature = "cloud")]
    fn text_action_dm_peer(&mut self, ch: char, action: TextAction) {
        let submit = |chart: &mut ChartApp| {
            let wid = WidgetId::from(crate::text_input::DM_PEER);
            let top_public_id = chart
                .panel_app
                .community_state
                .dm
                .lookup_results
                .first()
                .map(|h| h.public_id.clone());
            let with_label = match top_public_id {
                Some(id) => id,
                None => {
                    let typed = chart.input_coordinator.borrow().text_fields().text(&wid).trim().to_string();
                    if typed.is_empty() {
                        return;
                    }
                    typed
                }
            };
            chart.input_coordinator.borrow_mut().text_fields_mut().clear(&wid);
            let dm = &mut chart.panel_app.community_state.dm;
            dm.close_lookup_dropdown();
            dm.last_error = None;
            dm.request = Some(mlc_app_layout::ui::modal_settings::DmRequest::Open { with_label });
        };
        if ch == '\r' || ch == '\n' {
            submit(self);
            return;
        }
        if ch == '\x1b' {
            self.panel_app.community_state.dm.close_lookup_dropdown();
            self.input_coordinator.borrow_mut().text_fields_mut().blur();
            return;
        }
        match action {
            TextAction::Commit(_) => submit(self),
            TextAction::Cancel => {
                self.panel_app.community_state.dm.close_lookup_dropdown();
            }
            _ => {
                self.panel_app.community_state.dm.arm_lookup_debounce();
            }
        }
    }

    /// The chat-list header's own name filter (messenger UI-2b,
    /// `messenger_overlay.rs`'s `draw_chat_filter_row`). No debounce, no
    /// server round trip, no `Commit`/`Cancel` submit action — filtering is
    /// a client-side substring match `draw_chat_list` re-reads straight off
    /// the field's own live text every frame, so every keystroke here is
    /// already "applied" the moment `TextFieldStore::on_char` updates the
    /// buffer; the only thing THIS function still owns is Escape (clear +
    /// blur, same idiom every other search-like field in this crate uses).
    #[cfg(feature = "cloud")]
    fn text_action_dm_chat_filter(&mut self, ch: char, action: TextAction) {
        let cancel = |chart: &mut ChartApp| {
            let wid = WidgetId::from(crate::text_input::DM_CHAT_FILTER);
            chart.input_coordinator.borrow_mut().text_fields_mut().clear(&wid);
            chart.input_coordinator.borrow_mut().text_fields_mut().blur();
        };
        if ch == '\x1b' {
            cancel(self);
            return;
        }
        if let TextAction::Cancel = action {
            cancel(self);
        }
    }

    /// Share settings nick box. The lookup reads the live field text every
    /// frame, same as the chat-list filter. Escape clears and blurs.
    #[cfg(feature = "cloud")]
    fn text_action_share_settings_nick(&mut self, ch: char, action: TextAction) {
        let cancel = |chart: &mut ChartApp| {
            let wid = WidgetId::from(crate::text_input::SHARE_SETTINGS_NICK);
            chart.input_coordinator.borrow_mut().text_fields_mut().clear(&wid);
            chart.input_coordinator.borrow_mut().text_fields_mut().blur();
            chart.panel_app.share_settings_state.hits.clear();
        };
        if ch == '\x1b' {
            cancel(self);
            return;
        }
        if let TextAction::Cancel = action {
            cancel(self);
        }
    }

    /// Friends-tab nick search. Escape clears the box and the hit list.
    fn text_action_friends_nick(&mut self, ch: char, action: TextAction) {
        let cancel = |chart: &mut ChartApp| {
            let wid = WidgetId::from(crate::text_input::FRIENDS_NICK);
            chart.input_coordinator.borrow_mut().text_fields_mut().clear(&wid);
            chart.input_coordinator.borrow_mut().text_fields_mut().blur();
            chart.panel_app.user_settings_state.friend_hits.clear();
        };
        if ch == '\x1b' {
            cancel(self);
            return;
        }
        if let TextAction::Cancel = action {
            cancel(self);
        }
    }

    /// The credentials modal's webhook URL field. Commits into the SAME
    /// `AlertSettingsState::notification_secrets.webhook.url` the alert
    /// settings modal's own (now read-only) webhook row displays — one
    /// value, edited from either surface, riding the existing
    /// `notification_settings_dirty` persist path rather than a second one.
    fn text_action_cred_webhook_url(&mut self, ch: char, action: TextAction) {
        // The field cannot be focused while the vault is locked (the render
        // layer never registers its hit zone then) — defense in depth
        // against a stray focus/key event, same as `text_action_alert_token`.
        if !self.panel_app.credentials_state.secrets_available {
            self.input_coordinator.borrow_mut().text_fields_mut().blur();
            return;
        }
        let commit = |chart: &mut ChartApp| {
            let wid = WidgetId::from(crate::text_input::CRED_WEBHOOK_URL);
            let url = chart.input_coordinator.borrow().text_fields().text(&wid).trim().to_string();
            chart.panel_app.alert_settings_state.notification_secrets.webhook.url = url;
            chart.panel_app.alert_settings_state.notification_settings_dirty = true;
            chart.input_coordinator.borrow_mut().text_fields_mut().blur();
            chart.panel_app.credentials_state.webhook_url_focused = false;
        };
        if ch == '\r' || ch == '\n' || ch == '\x1b' {
            commit(self);
            return;
        }
        if matches!(action, TextAction::Commit(_) | TextAction::Cancel) {
            commit(self);
        }
    }

    /// One of the credentials modal's "add key" form fields. Tab chains to
    /// the next field (wrapping), Escape abandons the form back to the
    /// list, and Enter attempts to submit — same `try_add_exchange_key` the
    /// "Add" button calls, so Enter-from-any-field and click-Add behave
    /// identically.
    fn text_action_cred_add_field(&mut self, field: crate::input::credentials::CredAddField, ch: char, action: TextAction) {
        match ch {
            '\x09' => {
                self.focus_credentials_add_field(field.next());
                return;
            }
            '\x1b' => {
                self.panel_app.credentials_state.view =
                    mlc_app_layout::ui::modal_settings::CredentialsView::List;
                self.panel_app.credentials_state.error_message = None;
                self.panel_app.credentials_state.clear_add_form();
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
                return;
            }
            _ => {}
        }
        if let TextAction::Commit(_) = action {
            self.try_add_exchange_key();
        }
    }

    fn apply_watermark_text(&mut self, text: &str) {
        let all_pids: Vec<_> = self.panel_app.panel_grid.containers().values().map(|w| w.chart_panel_id).collect();
        for pid in all_pids {
            let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
            match &mut panel.watermark {
                Some(wm) => wm.set_text(text.to_string()),
                None => {
                    let mut wm = mlc_core::Watermark::default();
                    wm.set_text(text.to_string());
                    panel.watermark = Some(wm);
                }
            }
        }
    }

    fn text_action_watermark_text(&mut self, action: TextAction) {
        match action {
            TextAction::Commit(text) => {
                self.apply_watermark_text(&text);
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
                self.autosave_snapshot();
            }
            TextAction::TextChanged(text) => {
                self.apply_watermark_text(&text);
            }
            TextAction::Cancel => {
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
            }
            TextAction::None | TextAction::RawInput(_) => {}
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

    /// Commit handler for the `symbol_selector` toolbar dropdown's inline
    /// fast-search editor (symbol-search-compare-inner arc, Wave 2, кусок 4
    /// — owner spec 2026-07-16). Unlike `text_action_tf_favorite_input`,
    /// there is no commit-time validation grammar — any text is a valid
    /// substring query (mirrors `text_action_search_overlay`'s
    /// `TextChanged` contract for the full-screen modal). `TextChanged`
    /// only updates the live query text/cursor; the result-row rebuild
    /// itself happens per-frame in `ChartApp::symbol_quick_search_render`
    /// (reads `self.symbol_quick_edit.text` fresh every frame — no
    /// separate results cache to keep in sync here).
    fn text_action_symbol_quick_search(&mut self, action: TextAction) {
        match action {
            TextAction::Commit(_) => {
                // Enter applies the first result row exactly as if it had
                // been clicked. Which apply path fires depends on which
                // surface opened the editor (trading-header-symbol-edit arc,
                // 2026-07-25 — see `SymbolQuickEditTarget`).
                let target = self.symbol_quick_edit.as_ref().map(|e| e.target);
                match target {
                    Some(crate::SymbolQuickEditTarget::TradingPanel(pid)) => {
                        let first = self.symbol_quick_search_render()
                            .and_then(|r| r.results.first().cloned());
                        if let Some(res) = first {
                            let resolved_exchange = self.core.exchange_symbols()
                                .find_by_label(&res.exchange_id)
                                .unwrap_or(self.active_exchange);
                            self.commit_symbol_search_to_trading_panel(
                                pid, &res.symbol, resolved_exchange.as_str(), &res.account_type,
                            );
                        }
                        self.close_symbol_quick_dropdown();
                    }
                    Some(crate::SymbolQuickEditTarget::SectionPanel(pid)) => {
                        // Data-section header editor (Stage B) — group
                        // instrument write + apply-key rebind.
                        let first = self.symbol_quick_search_render()
                            .and_then(|r| r.results.first().cloned());
                        if let Some(res) = first {
                            let resolved_exchange = self.core.exchange_symbols()
                                .find_by_label(&res.exchange_id)
                                .unwrap_or(self.active_exchange);
                            self.commit_symbol_search_to_section_panel(
                                pid, &res.symbol, resolved_exchange.as_str(), &res.account_type,
                            );
                        }
                        self.close_symbol_quick_dropdown();
                    }
                    Some(crate::SymbolQuickEditTarget::MultiDomRoster(pid)) => {
                        // Enter applies the first NON-roster result row (the
                        // roster block sits on top of the list); the dropdown
                        // stays open — roster editing is a multi-click
                        // session, Escape / click-away closes it.
                        let (base, roster) = self.panels_store.dom.get(&pid)
                            .map(|s| (s.multi.symbol.clone(), s.multi.config.exchanges.clone()))
                            .unwrap_or_default();
                        let first = self.symbol_quick_search_render()
                            .map(|r| r.results)
                            .unwrap_or_default()
                            .into_iter()
                            .find(|res| {
                                use mlc_app_panels::trading::order_flow::multi_dom;
                                !(res.symbol.eq_ignore_ascii_case(&base)
                                    && roster.iter().any(|e| {
                                        multi_dom::col_matches(e, &res.exchange_id, &res.account_type)
                                    }))
                            });
                        if let Some(res) = first {
                            self.toggle_multi_dom_roster(pid, &res.symbol, &res.exchange_id, &res.account_type);
                        }
                    }
                    Some(crate::SymbolQuickEditTarget::MultiTapeRoster(pid)) => {
                        // Same Enter contract as the Multi-DOM roster:
                        // apply the first non-roster result, keep the
                        // dropdown open (multi-click session).
                        let (base, roster) = {
                            let ps = &self.panels_store;
                            ps.trade_tape.get(&pid).map(|s| (s.multi.symbol.clone(), s.multi.entries.clone()))
                                .or_else(|| ps.l2_tape.get(&pid).map(|s| (s.multi.symbol.clone(), s.multi.entries.clone())))
                                .or_else(|| ps.big_trades.get(&pid).map(|s| (s.multi.symbol.clone(), s.multi.entries.clone())))
                                .unwrap_or_default()
                        };
                        let first = self.symbol_quick_search_render()
                            .map(|r| r.results)
                            .unwrap_or_default()
                            .into_iter()
                            .find(|res| {
                                use mlc_app_panels::trading::order_flow::multi_dom;
                                !(res.symbol.eq_ignore_ascii_case(&base)
                                    && roster.iter().any(|e| {
                                        multi_dom::col_matches(e, &res.exchange_id, &res.account_type)
                                    }))
                            });
                        if let Some(res) = first {
                            self.toggle_multi_tape_roster(pid, &res.symbol, &res.exchange_id, &res.account_type);
                        }
                    }
                    // `TopbarChart` (or no live editor, defensive) — same
                    // dispatch `handle_search_modal_click`'s
                    // Commit-via-first-result contract uses for the
                    // full-screen modal (`text_action_search_overlay`). Bug
                    // fix (Wave 4 audit): the composite key must carry the
                    // full `SYMBOL_QUICK_ITEM_PREFIX` (`"sym_quick:item:"`),
                    // not a bare `"item:"` — `handle_symbol_quick_click` only
                    // strips the former, so Enter silently no-op'd before
                    // this fix.
                    _ => {
                        let first_key = self.symbol_quick_search_render()
                            .and_then(|r| r.results.first().map(|res| {
                                format!("{}:{}:{}", res.symbol, res.exchange_id, res.account_type)
                            }));
                        if let Some(key) = first_key {
                            let rest = format!("{}{}", mlc_app_layout::panel_app::SYMBOL_QUICK_ITEM_PREFIX, key);
                            self.handle_symbol_quick_click(&rest);
                        } else {
                            self.close_symbol_quick_dropdown();
                        }
                    }
                }
            }
            TextAction::TextChanged(ref new_text) => {
                let field_id = uzor::WidgetId::from(crate::text_input::SYMBOL_QUICK_SEARCH);
                let (cursor, selection) = {
                    let tf = self.input_coordinator.borrow();
                    let store = tf.text_fields();
                    (store.cursor(&field_id), store.selection_range(&field_id))
                };
                if let Some(ref mut edit) = self.symbol_quick_edit {
                    edit.text = new_text.clone();
                    edit.cursor = cursor;
                    edit.selection = selection;
                }
            }
            TextAction::Cancel => {
                self.close_symbol_quick_dropdown();
            }
            TextAction::None | TextAction::RawInput(_) => {}
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
                // "create" — the shell names a creator dialog's primary
                // action `create`, not `save` (see each `handle_*_tmpl_modal_click`'s
                // own match arms); Enter must land on the exact same arm the
                // "Save" button's click does, or it silently falls through
                // to the `_ => unhandled` arm and never saves anything.
                // "create"/"close" never read (x, y) — only the "input"
                // arm does — so the keyboard path passes no real coordinates.
                if id == PRIMITIVE_SETTINGS_TEMPLATE_NAME {
                    self.handle_prim_tmpl_modal_click("create", 0.0, 0.0);
                } else if id == INDICATOR_SETTINGS_TEMPLATE_NAME {
                    self.handle_ind_tmpl_modal_click("create", 0.0, 0.0);
                } else if id == COMPARE_SETTINGS_TEMPLATE_NAME {
                    self.handle_cmp_tmpl_modal_click("create", 0.0, 0.0);
                } else if id == CHART_SETTINGS_TEMPLATE_NAME {
                    self.handle_chart_tmpl_modal_click("create", 0.0, 0.0);
                }
            }
            // Escape in the name field takes the same exit the ✕ does — the
            // dialogs no longer have a Cancel to route it through.
            TextAction::Cancel => {
                if id == PRIMITIVE_SETTINGS_TEMPLATE_NAME {
                    self.handle_prim_tmpl_modal_click("close", 0.0, 0.0);
                } else if id == INDICATOR_SETTINGS_TEMPLATE_NAME {
                    self.handle_ind_tmpl_modal_click("close", 0.0, 0.0);
                } else if id == COMPARE_SETTINGS_TEMPLATE_NAME {
                    self.handle_cmp_tmpl_modal_click("close", 0.0, 0.0);
                } else if id == CHART_SETTINGS_TEMPLATE_NAME {
                    self.handle_chart_tmpl_modal_click("close", 0.0, 0.0);
                }
            }
            TextAction::TextChanged(_) | TextAction::None | TextAction::RawInput(_) => {}
        }
    }
}
