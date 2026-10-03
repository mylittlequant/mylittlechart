//! `ChartApp` glue of the settings modal's **Admin** tab.
//!
//! State and logic live in `mlc_app_layout::ui::admin_tab`; the tab is drawn
//! by `layout::modals::user_settings_admin`. This file is what only the app
//! can do: read the ten text inputs out of the `TextFieldStore`, put codes
//! on the clipboard (`ChartApp::clipboard_text`, the path the shell already
//! drains for the recovery key and the friend-invite link), and give the shell
//! one small surface to drive — [`ChartApp::take_admin_request`] out,
//! [`ChartApp::apply_admin_outcome`] in — so the shell never touches the
//! tab's fields.

use mlc_app_layout::ui::admin_tab::{AdminAction, AdminField, AdminOutcome, AdminRequest};
use uzor::input::TextAction;
use uzor::WidgetId;

use crate::ChartApp;

impl ChartApp {
    /// Tell the tab whether this shell can serve `/api/staff/*` requests. The
    /// web shell sets it every frame; a shell that never does keeps the tab
    /// out of the rail (there would be nothing behind its buttons).
    pub fn set_admin_served(&mut self, served: bool) {
        self.panel_app.user_settings_state.admin.served = served;
    }

    /// The Admin request the shell should perform next, if any. Also queues
    /// the first batches load when the tab is on screen.
    pub fn take_admin_request(&mut self) -> Option<AdminRequest> {
        self.panel_app.user_settings_state.take_admin_request()
    }

    /// Hand a settled request back to the tab.
    pub fn apply_admin_outcome(&mut self, outcome: AdminOutcome) {
        self.panel_app.user_settings_state.admin.apply(outcome);
    }

    /// The account left the admin view (signed out, or the role went away):
    /// drop the tab's data, step off it, and empty its text inputs.
    pub fn drop_admin_view(&mut self) {
        self.panel_app.user_settings_state.drop_admin_view();
        let mut coord = self.input_coordinator.borrow_mut();
        let tf = coord.text_fields_mut();
        let focused_here = tf.focused().is_some_and(|w| AdminField::from_id(w.as_str()).is_some());
        if focused_here {
            tf.blur();
        }
        for field in AdminField::ALL {
            tf.clear(&WidgetId::from(field.id()));
        }
    }

    /// The account's roles were just (re)fetched. Someone who no longer holds
    /// `admin` loses the tab and its data at once.
    pub fn admin_roles_refreshed(&mut self) {
        if !self.panel_app.user_settings_state.is_admin() {
            self.drop_admin_view();
        }
    }

    fn admin_field_text(&self, field: AdminField) -> String {
        let wid = WidgetId::from(field.id());
        self.input_coordinator.borrow().text_fields().text(&wid).to_string()
    }

    /// One click on the Admin tab (`user_settings:admin:*`). Forms are read
    /// out of the text fields, validated by the tab's state, and either armed
    /// for confirmation or queued for the shell.
    pub(crate) fn admin_click(&mut self, action: AdminAction) {
        // A stale hit id must never act for anyone the tab is hidden from.
        if !self.panel_app.user_settings_state.admin_tab_visible() {
            return;
        }
        match action {
            // Claimed on mouse-down by the store, like the Account tab's boxes.
            AdminAction::Field(_) => {}
            AdminAction::Section(section) => self.panel_app.user_settings_state.admin.set_section(section),
            AdminAction::Mint => {
                let days = self.admin_field_text(AdminField::MintDays);
                let count = self.admin_field_text(AdminField::MintCount);
                let note = self.admin_field_text(AdminField::MintNote);
                self.panel_app.user_settings_state.admin.submit_mint(&days, &count, &note);
            }
            AdminAction::CopyAll => {
                let text = self.panel_app.user_settings_state.admin.copy_all_text();
                self.admin_copy(text);
            }
            AdminAction::CopyOne(index) => {
                let text = self.panel_app.user_settings_state.admin.copy_one_text(index);
                self.admin_copy(text);
            }
            AdminAction::DismissCodes => self.panel_app.user_settings_state.admin.dismiss_minted(),
            AdminAction::RefreshBatches => self.panel_app.user_settings_state.admin.refresh_batches(),
            AdminAction::RevokeBatch(batch_id) => {
                self.panel_app.user_settings_state.admin.arm_revoke_batch(&batch_id);
            }
            AdminAction::ConfirmYes => self.panel_app.user_settings_state.admin.confirm_yes(),
            AdminAction::ConfirmNo => self.panel_app.user_settings_state.admin.confirm_no(),
            AdminAction::PaidDays => {
                let nick = self.admin_field_text(AdminField::GrantNick);
                let days = self.admin_field_text(AdminField::GrantDays);
                self.panel_app.user_settings_state.admin.arm_paid_days(&nick, &days);
            }
            AdminAction::LifetimeGrant(kind) => {
                let nick = self.admin_field_text(AdminField::GrantNick);
                let note = self.admin_field_text(AdminField::GrantNote);
                self.panel_app.user_settings_state.admin.arm_lifetime_grant(&nick, kind, &note);
            }
            AdminAction::LifetimeRevoke(kind) => {
                let nick = self.admin_field_text(AdminField::GrantNick);
                self.panel_app.user_settings_state.admin.arm_lifetime_revoke(&nick, kind);
            }
            AdminAction::Lookup => {
                let nick = self.admin_field_text(AdminField::UserNick);
                self.panel_app.user_settings_state.admin.submit_lookup(&nick);
            }
            AdminAction::IssueInvite => {
                let nick = self.admin_field_text(AdminField::InviteNick);
                let days = self.admin_field_text(AdminField::InviteDays);
                let note = self.admin_field_text(AdminField::InviteNote);
                self.panel_app.user_settings_state.admin.submit_invite(&nick, &days, &note);
            }
            AdminAction::CopyInvite => {
                let text = self.panel_app.user_settings_state.admin.copy_invite_text();
                self.admin_copy(text);
            }
            AdminAction::DismissInvite => self.panel_app.user_settings_state.admin.dismiss_invite(),
            AdminAction::RefreshInvites => self.panel_app.user_settings_state.admin.refresh_invites(),
            AdminAction::SetLoginChallenge(on) => self.panel_app.user_settings_state.admin.set_login_challenge(on),
            AdminAction::RevokeInvite(invite_id) => {
                self.panel_app.user_settings_state.admin.arm_revoke_invite(&invite_id);
            }
        }
    }

    /// Put `text` on the clipboard through the app's existing path and say so.
    fn admin_copy(&mut self, text: Option<String>) {
        if let Some(text) = text {
            self.clipboard_text = Some(text);
            self.panel_app.user_settings_state.admin.note_copied();
        }
    }

    /// Enter / Escape in one of the Admin tab's inputs. Escape leaves the
    /// field. Enter presses the button that field belongs to — Mint, Add paid
    /// days, Look up, Issue invite — and does nothing in the two Grants fields that serve
    /// several buttons, so a stray Enter can never pick a grant for the admin.
    pub(crate) fn text_action_admin_field(&mut self, field: AdminField, ch: char, action: TextAction) {
        if ch == '\x1b' || matches!(action, TextAction::Cancel) {
            self.input_coordinator.borrow_mut().text_fields_mut().blur();
            return;
        }
        let submitted = ch == '\r' || ch == '\n' || matches!(action, TextAction::Commit(_));
        if !submitted {
            return;
        }
        let press = match field {
            AdminField::MintDays | AdminField::MintCount | AdminField::MintNote => Some(AdminAction::Mint),
            AdminField::GrantDays => Some(AdminAction::PaidDays),
            AdminField::UserNick => Some(AdminAction::Lookup),
            AdminField::InviteNick | AdminField::InviteDays | AdminField::InviteNote => Some(AdminAction::IssueInvite),
            AdminField::GrantNick | AdminField::GrantNote => None,
        };
        if let Some(press) = press {
            self.admin_click(press);
        }
    }
}
