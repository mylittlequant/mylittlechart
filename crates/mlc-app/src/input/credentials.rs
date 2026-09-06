//! Vault tab — click handling + the vault lock/unlock actions it drives.
//!
//! The vault surface (exchange API keys + the alert webhook URL) used to be
//! a standalone modal; it is the `Vault` tab of the User Settings modal now
//! (`mlc-app-layout`'s `layout::modals::user_settings::render_vault_tab`) —
//! opening it used to CLOSE user settings, a modal stacked on a modal with
//! no honest back button. `CredentialsState` (the tab's own state) kept its
//! name across the move; only its former modal-chrome fields (`is_open`,
//! `position`, `is_dragging`, `drag_offset`) are gone.
//!
//! # Which vault-key source this tab drives off
//!
//! `mlc-vault::store::SecretStore` is the newer, TTL-bounded, zeroize-on-seal
//! design — but nothing in this app constructs one yet, and swapping
//! `BackendCore::UserProfileManager::vault_key` for it is its own piece of
//! work, out of scope here. This tab drives off the vault key that IS
//! already wired end to end: `self.core.user().vault_key()` (the same
//! `Option<VaultKey>` `AlertSettingsState::secrets_available` already
//! mirrors from `ChartApp::apply_profile_state`). There is therefore no TTL
//! to show — see `render_vault_tab` (the render side, in `mlc-app-layout`)
//! for why that is a deliberate absence, not an oversight.
//!
//! # Issuing (or removing) a key while sealed
//!
//! Both are mutations that must reach `vault.enc` to survive the next
//! unlock's `VaultSecrets::merge_into` (which OVERWRITES
//! `profile.domain.exchange_keys` wholesale from whatever is on disk) — a
//! sealed-state edit would sit in RAM only and be silently discarded the
//! next time the vault unlocks. So neither is offered while sealed:
//! "+ Add key" instead routes to `CredentialsView::UnlockPassphrase` (the
//! same shared vault page the standalone profile-manager modal draws for
//! native boot, just hosted in this tab — see `panel_click.rs`'s
//! `"profile_mgr:unlock"` arm for how it tells this case apart from an
//! ordinary boot-time/profile-switch unlock) and remembers to switch back to
//! the add form once the unlock actually succeeds (`pending_unlock_return`).
//! Remove is simply not offered while sealed — same hazard, no explicit
//! requirement to route it through unlock too, so it stays the smaller
//! surface.

use mlc_core::user_profile::profile::StoredExchangeKey;
use mlc_app_layout::ui::modal_settings::{CredentialsView, UserSettingsTab};

use crate::ChartApp;
use crate::text_input;

impl ChartApp {
    /// Handle clicks on widgets registered with the "cred:" prefix. The
    /// vault surface has no modal chrome of its own anymore (it is the
    /// Vault tab of the User Settings modal) — there is no `"close"`,
    /// `"modal_bg"`, `"header"` or `"dimmer"` here to catch; those all
    /// belong to `"user_settings:"`'s own arms now.
    pub(super) fn handle_credentials_click(&mut self, rest: &str) {
        match rest {
            "lock" => self.lock_vault(),
            "unlock" => self.route_credentials_to_unlock(CredentialsView::List),
            "add_key" => {
                if self.panel_app.credentials_state.secrets_available {
                    self.panel_app.credentials_state.view = CredentialsView::AddKey;
                    self.panel_app.credentials_state.error_message = None;
                    self.focus_credentials_add_field(CredAddField::Exchange);
                } else {
                    self.route_credentials_to_unlock(CredentialsView::AddKey);
                }
            }
            rest if rest.starts_with("remove:") => {
                if self.panel_app.credentials_state.secrets_available {
                    if let Ok(idx) = rest["remove:".len()..].parse::<usize>() {
                        self.remove_exchange_key(idx);
                    }
                }
            }
            // A plain click on any of these six text fields already reached
            // `ChartApp::on_drag_start` (mouse-down fires before this click
            // dispatch, which only runs on mouse-up) — that is what places
            // the caret AND sets the matching `CredentialsState::*_focused`
            // bool, from the field's own click-x, not a canned "focus at
            // whatever the cursor already was" call. Routing this arm to
            // `focus_credentials_add_field`/`.focus()` the way it used to
            // would call `TextFieldStore::focus` a SECOND time on a field
            // that is already the focused one, which silently un-engages it
            // (`TextFieldState::engaged` — see its own doc) and takes the
            // caret and the reveal ring away right after they appeared.
            // `focus_credentials_add_field` itself is still very much alive
            // for the two paths that have no click position to place a
            // caret from: Tab-chaining between fields (`CredAddField::next`)
            // and "Add key" seeding the Exchange field on a fresh open.
            "webhook_url" | "add:exchange" | "add:label" | "add:api_key"
            | "add:api_secret" | "add:passphrase" => {}
            "add:back" => {
                self.panel_app.credentials_state.view = CredentialsView::List;
                self.panel_app.credentials_state.error_message = None;
                self.panel_app.credentials_state.clear_add_form();
                self.blur_credentials_add_fields();
            }
            "add:testnet" => {
                self.panel_app.credentials_state.add_testnet = !self.panel_app.credentials_state.add_testnet;
            }
            // Shared by the API-secret and passphrase fields on this form.
            "add:reveal" => {
                self.panel_app.credentials_state.secret_revealed =
                    !self.panel_app.credentials_state.secret_revealed;
            }
            "create" => self.try_add_exchange_key(),
            _ => {
                eprintln!("[ChartApp] credentials unhandled: {}", rest);
            }
        }
    }

    /// Open the User Settings modal on its Vault tab — the vault surface's
    /// entry point now that it is a tab rather than a modal stacked on top
    /// of this one (the "navigation hell" the owner called out: opening the
    /// credentials modal used to CLOSE user settings, with no way back).
    pub(super) fn open_vault_tab(&mut self) {
        self.sync_credentials_from_core();
        self.panel_app.credentials_state.open();
        self.panel_app.user_settings_state.active_tab = UserSettingsTab::Vault;
        self.panel_app.user_settings_state.is_open = true;
    }

    /// Refresh the credentials modal's cache from `BackendCore` — the same
    /// cache-refreshed-by-the-app-layer pattern `apply_profile_state` uses
    /// for `AlertSettingsState`. Call whenever the vault key or the
    /// exchange-key list may have changed underneath the modal (open, after
    /// add/remove, after lock, after a mid-session unlock).
    pub(crate) fn sync_credentials_from_core(&mut self) {
        let secrets_available = self.core.user().vault_key().is_some();
        let exchange_keys = self.core.user().profile().domain.exchange_keys.clone();
        let runtime_id = self.panel_app.user_settings_state.runtime_profile_id.clone();
        let vault_exists = self
            .panel_app
            .user_settings_state
            .profiles_with_vault_status
            .iter()
            .find(|(id, _, _, _)| *id == runtime_id)
            .map(|(_, _, _, has_vault)| *has_vault)
            .unwrap_or(false);
        self.panel_app.credentials_state.secrets_available = secrets_available;
        self.panel_app.credentials_state.vault_exists = vault_exists;
        self.panel_app.credentials_state.exchange_keys = exchange_keys;

        // Seed the webhook URL field from the same live buffer
        // `AlertSettingsState`'s own notifications tab reads and writes —
        // one value, edited from either surface, never two copies to drift.
        let webhook_url = if secrets_available {
            self.panel_app.alert_settings_state.notification_secrets.webhook.url.clone()
        } else {
            String::new()
        };
        let wid = uzor::WidgetId::from(text_input::CRED_WEBHOOK_URL);
        self.input_coordinator.borrow_mut().text_fields_mut().set_text(&wid, &webhook_url);
    }

    /// Seal the wired agent-key `SecretStore` (if the host set one via
    /// [`ChartApp::set_agent_key_store`]) — the counterpart of
    /// [`Self::sync_agent_key_store`], called from every path that seals
    /// the main vault so the Agent API's key store never outlives it.
    fn seal_agent_key_store(&self) {
        if let Some(store) = &self.agent_key_store {
            store.seal();
        }
    }

    /// Refresh the wired agent-key `SecretStore` (if any) off the SAME
    /// `VaultKey` that just unlocked the main vault — called from every
    /// path that derives or restores one (`finish_credentials_unlock`,
    /// `apply_vault_setup`'s fresh-setup branch, `apply_recovery_unlock`).
    /// `unlock_with_key` skips PBKDF2 entirely (the render thread already
    /// holds the derived key here, never a passphrase to re-derive from).
    fn sync_agent_key_store(&self, vault_key: &mlc_vault::vault::VaultKey) {
        if let Some(store) = &self.agent_key_store {
            if let Err(e) = store.unlock_with_key(vault_key) {
                eprintln!("[ChartApp] agent key store: failed to unlock alongside vault: {}", e);
            }
        }
    }

    /// Seal the vault deliberately (the "Lock" button) — the counterpart a
    /// user must have alongside "Unlock" per the owner's ask, since until
    /// now there was only ever unlock.
    fn lock_vault(&mut self) {
        self.core.user_mut().seal_vault();
        self.seal_agent_key_store();

        // The alert-settings mirror holds its own copy of the same secrets
        // (telegram bot token, webhook url) — sealing must wipe it too, and
        // the TextFieldStore buffers that back its editable fields, or a
        // sealed vault would still show a token/URL it claims to have
        // forgotten.
        self.panel_app.alert_settings_state.secrets_available = false;
        self.panel_app.alert_settings_state.notification_secrets = Default::default();
        {
            let mut ic = self.input_coordinator.borrow_mut();
            let tf = ic.text_fields_mut();
            tf.set_text(&uzor::WidgetId::from(text_input::ALERT_TG_TOKEN), "");
            tf.set_text(&uzor::WidgetId::from(text_input::CRED_WEBHOOK_URL), "");
            tf.blur();
        }
        self.panel_app.alert_settings_state.tg_token_render = Default::default();
        self.panel_app.credentials_state.webhook_url_focused = false;
        self.panel_app.credentials_state.view = CredentialsView::List;
        self.sync_credentials_from_core();
        eprintln!("[ChartApp] credentials: vault sealed");
    }

    /// Route to the existing vault page, remembering to come back to
    /// `return_to` on this same tab once it succeeds. See the module doc for
    /// why this reuses the profile manager's page BODIES rather than growing
    /// a second prompt of its own.
    ///
    /// Which page depends on whether the profile has a vault at all. A profile
    /// that has none must land on `CreatePassphrase` — two fields, a
    /// confirmation, and the recovery key that follows it. Sending it to
    /// `UnlockPassphrase` asks a single field for a passphrase that protects
    /// nothing, and no vault is ever created.
    ///
    /// This ALWAYS lands on the Vault tab's own page (`CredentialsState::
    /// view`), never on the standalone profile-manager modal
    /// (`show_profile_manager`) — most callers are already inside that tab
    /// (the "Unlock"/"Set up" status button, or "+ Add key" while sealed),
    /// so popping the standalone modal on top of it would be exactly the
    /// modal-on-modal navigation dead end the tab move exists to close. The
    /// one caller that is NOT already on the Vault tab — the Server tab's
    /// "Set up a vault passphrase" button, offered when there is no vault to
    /// mint an agent key against (agent-api-auth-and-secret-store plan,
    /// piece 4 follow-up) — is fine too: this function switches
    /// `active_tab` to `Vault` itself below, unconditionally.
    pub(super) fn route_credentials_to_unlock(&mut self, return_to: CredentialsView) {
        self.panel_app.credentials_state.pending_unlock_return = Some(return_to);

        let target_id = self.panel_app.user_settings_state.runtime_profile_id.clone();
        let (has_vault, target_name) = self
            .panel_app
            .user_settings_state
            .profiles_with_vault_status
            .iter()
            .find(|(id, _, _, _)| *id == target_id)
            .map(|(_, name, _, has_vault)| (*has_vault, name.clone()))
            .unwrap_or((false, String::new()));

        self.panel_app.credentials_state.view = if has_vault {
            CredentialsView::UnlockPassphrase
        } else {
            CredentialsView::CreatePassphrase
        };

        // Leaving the wizard counts as having seen it, even when what closes
        // it is the flow moving on to the vault page rather than the user
        // dismissing it. Without this the flag stays false for the life of
        // the profile and the wizard greets every launch — which is exactly
        // what it did (owner, 2026-09-03: "а нахуй мне постоянно открывают
        // меню выбора темы?"). Called before the `uss` borrow below because
        // it needs `self`.
        self.mark_welcome_wizard_seen();

        let uss = &mut self.panel_app.user_settings_state;
        uss.profile_manager_target_id = target_id;
        uss.profile_manager_target_name = target_name;
        uss.vault_unlock_error = None;
        uss.vault_unlock_attempts = 0;
        uss.set_passphrase_error = String::new();
        uss.active_tab = UserSettingsTab::Vault;
        uss.is_open = true;
        uss.show_welcome_wizard = false;
        uss.new_passphrase_focused = false;
        uss.confirm_passphrase_focused = false;
        // A leftover reveal from a previous visit to this page must not
        // greet the next passphrase typed into it.
        uss.passphrase_revealed = false;
        {
            let mut ic = self.input_coordinator.borrow_mut();
            let tf = ic.text_fields_mut();
            tf.clear(&uzor::WidgetId::from(text_input::NEW_PASSPHRASE));
            tf.clear(&uzor::WidgetId::from(text_input::CONFIRM_PASSPHRASE));
            tf.blur();
        }
        eprintln!(
            "[ChartApp] credentials: routing to {}, will return to {:?}",
            if has_vault { "unlock" } else { "vault setup" },
            return_to
        );
    }

    /// Remove one exchange key by index — mutates the live profile directly
    /// (same "mutate `core.user_mut().profile_mut()`, then `persist_profile`"
    /// idiom `granularity_favorites` already uses), then refreshes the
    /// modal's cache so the row disappears this frame.
    fn remove_exchange_key(&mut self, idx: usize) {
        {
            let mut user_mut = self.core.user_mut();
            let keys = &mut user_mut.profile_mut().domain.exchange_keys;
            if idx >= keys.len() {
                return;
            }
            keys.remove(idx);
        }
        self.persist_profile();
        self.sync_credentials_from_core();
    }

    /// Validate and commit the "add key" form — the `"create"` click and
    /// the Enter-key path (`text_action_cred_add_field`) both call this.
    pub(crate) fn try_add_exchange_key(&mut self) {
        let (exchange_id, label, api_key, api_secret, passphrase) = {
            let ic = self.input_coordinator.borrow();
            let tf = ic.text_fields();
            (
                tf.text(&uzor::WidgetId::from(text_input::CRED_ADD_EXCHANGE)).trim().to_lowercase(),
                tf.text(&uzor::WidgetId::from(text_input::CRED_ADD_LABEL)).trim().to_string(),
                tf.text(&uzor::WidgetId::from(text_input::CRED_ADD_API_KEY)).trim().to_string(),
                tf.text(&uzor::WidgetId::from(text_input::CRED_ADD_API_SECRET)).to_string(),
                tf.text(&uzor::WidgetId::from(text_input::CRED_ADD_PASSPHRASE)).to_string(),
            )
        };

        if exchange_id.is_empty() || api_key.is_empty() {
            self.panel_app.credentials_state.error_message =
                Some("Exchange and API key are required.".to_string());
            return;
        }

        let key = StoredExchangeKey {
            exchange_id,
            label,
            api_key,
            api_secret,
            passphrase: if passphrase.is_empty() { None } else { Some(passphrase) },
            keychain_ref: None,
            passphrase_keychain_ref: None,
            testnet: self.panel_app.credentials_state.add_testnet,
            created_at: mlc_core::preset::preset::unix_now_secs(),
        };

        self.core.user_mut().profile_mut().domain.exchange_keys.push(key);
        self.persist_profile();

        self.panel_app.credentials_state.clear_add_form();
        self.panel_app.credentials_state.view = CredentialsView::List;
        self.panel_app.credentials_state.error_message = None;
        self.blur_credentials_add_fields();
        self.sync_credentials_from_core();
        eprintln!("[ChartApp] credentials: exchange key added");
    }

    /// Re-derive the vault key for the CURRENT profile and restore secret
    /// state from `vault.enc`, without rebuilding any window — the full
    /// implementation behind the `cred_unlock:{passphrase}` shell command,
    /// shared by both shells so neither duplicates the validate+decrypt+
    /// merge dance (`mlc_vault::vault`'s helpers are already storage-
    /// backend-agnostic — filesystem on native, `localStorage` on wasm —
    /// so this needs no `cfg` gate).
    pub fn try_unlock_credentials(&mut self, passphrase: &str) -> Result<(), String> {
        let dir = mlc_core::active_profile_data_dir();
        let salt_path = dir.join("salt.hex");
        let vault_path = dir.join("vault.enc");
        let salt = mlc_vault::vault::load_or_create_salt(&salt_path).map_err(|e| e.to_string())?;
        let key = mlc_vault::vault::derive_key(passphrase, &salt);
        let secrets = mlc_vault::vault::load_encrypted::<mlc_core::user_profile::VaultSecrets>(&key, &vault_path)
            .map_err(|_| "Wrong passphrase — please try again".to_string())?;
        self.finish_credentials_unlock(key, secrets);
        Ok(())
    }

    /// Apply a successful [`Self::try_unlock_credentials`]: set the key,
    /// merge the decrypted secrets into the live profile, refresh every
    /// cache that mirrors them, dismiss the unlock page, and — the whole
    /// point of `pending_unlock_return` — return to the Vault tab page
    /// (`List` or `AddKey`) that asked for the unlock.
    fn finish_credentials_unlock(&mut self, key: mlc_vault::vault::VaultKey, secrets: mlc_core::user_profile::VaultSecrets) {
        self.core.user_mut().unseal_vault(key, secrets);
        self.sync_agent_key_store(&key);

        let (notification_secrets, bot_token) = {
            let user = self.core.user();
            let ns = user.profile().domain.notification_secrets.clone();
            let token = ns.telegram.bot_token.clone();
            (ns, token)
        };
        self.panel_app.alert_settings_state.secrets_available = true;
        self.panel_app.alert_settings_state.notification_secrets = notification_secrets;
        {
            let tg_wid = uzor::WidgetId::from(text_input::ALERT_TG_TOKEN);
            self.input_coordinator.borrow_mut().text_fields_mut().set_text(&tg_wid, &bot_token);
        }

        self.sync_credentials_from_core();

        self.panel_app.user_settings_state.show_profile_manager = false;
        self.panel_app.user_settings_state.vault_unlock_error = None;

        if let Some(view) = self.panel_app.credentials_state.pending_unlock_return.take() {
            self.panel_app.user_settings_state.active_tab = UserSettingsTab::Vault;
            self.panel_app.user_settings_state.is_open = true;
            self.panel_app.credentials_state.view = view;
        }
        eprintln!("[ChartApp] credentials: vault unlocked mid-session");
    }

    /// Close out a vault-setup flow that ran to its end (the user
    /// acknowledged the recovery key).
    ///
    /// Two things have to happen here that nothing else does. The vault now
    /// exists, but `profiles_with_vault_status` is a snapshot the shells
    /// refresh on profile-list operations only — leaving it stale would make
    /// the Vault tab keep offering "Set up" for a vault that is already
    /// there, and route the next click back to the setup page. And if the
    /// setup was started from the Vault tab, that tab asked to be returned
    /// to; without this it just closes and the user is left on a bare chart
    /// wondering whether anything happened.
    pub fn finish_vault_setup(&mut self) {
        let runtime_id = self.panel_app.user_settings_state.runtime_profile_id.clone();
        for entry in self
            .panel_app
            .user_settings_state
            .profiles_with_vault_status
            .iter_mut()
        {
            if entry.0 == runtime_id {
                entry.3 = true;
            }
        }
        self.panel_app.user_settings_state.show_profile_manager = false;
        self.panel_app.user_settings_state.vault_unlock_error = None;
        self.sync_credentials_from_core();

        if let Some(view) = self.panel_app.credentials_state.pending_unlock_return.take() {
            self.panel_app.user_settings_state.active_tab = UserSettingsTab::Vault;
            self.panel_app.user_settings_state.is_open = true;
            self.panel_app.credentials_state.view = view;
            eprintln!("[ChartApp] credentials: vault created, returning to {:?}", view);
        }
    }

    /// Abandon whatever passphrase-entry page is showing — shared cleanup
    /// for `panel_click.rs`'s `"profile_mgr:back"` arm, run identically
    /// whichever of the two page-selector enums (`ProfileManagerPage` in
    /// the standalone modal, `CredentialsView` on the Vault tab) that arm
    /// is about to move away from. Mirrors `dismiss_profile_manager`'s own
    /// cleanup for the same reasons: a revealed passphrase must not survive
    /// navigating away, and a pending credentials-reseal return
    /// (`pending_unlock_return`) must not silently redirect some LATER,
    /// unrelated unlock.
    pub(crate) fn abandon_vault_page_entry(&mut self) {
        self.panel_app.user_settings_state.vault_unlock_error = None;
        self.panel_app.user_settings_state.vault_unlock_attempts = 0;
        {
            let np_wid = uzor::WidgetId::from(text_input::NEW_PASSPHRASE);
            self.input_coordinator.borrow_mut().text_fields_mut().clear(&np_wid);
            self.input_coordinator.borrow_mut().text_fields_mut().blur();
        }
        self.panel_app.user_settings_state.new_passphrase_focused = false;
        self.panel_app.user_settings_state.passphrase_revealed = false;
        self.panel_app.credentials_state.pending_unlock_return = None;
    }

    /// Blur whatever "add key" field is focused and clear its focus flags —
    /// shared by the Back button and a successful Add submit.
    fn blur_credentials_add_fields(&mut self) {
        let cs = &mut self.panel_app.credentials_state;
        cs.add_exchange_focused = false;
        cs.add_label_focused = false;
        cs.add_api_key_focused = false;
        cs.add_api_secret_focused = false;
        cs.add_passphrase_focused = false;
        self.input_coordinator.borrow_mut().text_fields_mut().blur();
    }

    /// Focus one "add key" form field, un-focusing the rest — same
    /// exclusive-focus idiom the profile-manager fields already use.
    pub(crate) fn focus_credentials_add_field(&mut self, field: CredAddField) {
        self.blur_credentials_add_fields();
        let (id, flag): (&str, &mut bool) = {
            let cs = &mut self.panel_app.credentials_state;
            match field {
                CredAddField::Exchange => (text_input::CRED_ADD_EXCHANGE, &mut cs.add_exchange_focused),
                CredAddField::Label => (text_input::CRED_ADD_LABEL, &mut cs.add_label_focused),
                CredAddField::ApiKey => (text_input::CRED_ADD_API_KEY, &mut cs.add_api_key_focused),
                CredAddField::ApiSecret => (text_input::CRED_ADD_API_SECRET, &mut cs.add_api_secret_focused),
                CredAddField::Passphrase => (text_input::CRED_ADD_PASSPHRASE, &mut cs.add_passphrase_focused),
            }
        };
        *flag = true;
        let wid = uzor::WidgetId::from(id);
        self.input_coordinator.borrow_mut().text_fields_mut().focus(wid);
    }
}

// =============================================================================
// Vault-family commands (e2e_setup / recovery_unlock / set_new_passphrase)
// =============================================================================
//
// Shared body for the vault-lifecycle commands `panel_click.rs` stages into
// `pending_updater_cmd` as `e2e_setup:{passphrase}` / `recovery_unlock:{key}`
// / `set_new_passphrase:{passphrase}`. Both shells own a `ProfileManager` at
// their own layer (see `user_manager::dispatch`'s module doc for why
// `ChartApp` never holds one itself) — these methods take it as a parameter,
// the same shape `ChartApp::dispatch_profile_command` already established
// for the five profile-family commands.
//
// What is deliberately NOT here: switching into a DIFFERENT, not-yet-active
// profile — native's "just created via the wizard, not switched yet" path
// and its "select a different existing profile from the list" path both
// tear down/rebuild OS windows and have no web equivalent yet
// (`ProfileEffect::SwitchRequested`'s doc has the fuller reasoning). Callers
// must compare the command's target profile id against the ACTIVE profile
// themselves and only reach these methods when they match; anything else is
// the caller's own business to route or decline.

/// Outcome of [`ChartApp::apply_vault_setup`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultSetupOutcome {
    /// The vault for the CURRENTLY ACTIVE profile is unlocked — `core.user()`
    /// / `core.templates()` already carry the new key, the caller must not
    /// re-derive or re-set it. `recovery_key` is `Some` only on first-time
    /// vault creation (a fresh recovery key was just generated): the caller
    /// MUST show it once — `ProfileManagerPage::ShowRecoveryKey` if the
    /// standalone modal is up (`UserSettingsState::show_profile_manager`),
    /// `CredentialsView::ShowRecoveryKey` on the Vault tab otherwise — before
    /// treating setup as complete.
    Ready { recovery_key: Option<String> },
    /// Existing-vault re-validation only: the passphrase did not decrypt
    /// `vault.enc`.
    WrongPassphrase(String),
    /// Salt/vault I/O failed (fresh-setup path only — re-validation's I/O
    /// errors surface as `WrongPassphrase`, via `try_unlock_credentials`).
    Error(String),
}

impl ChartApp {
    /// Create or re-validate the vault key for the CURRENTLY ACTIVE profile —
    /// the shared body of `e2e_setup:{passphrase}`'s two same-profile
    /// branches (native `main.rs`'s "vault already exists, validate" and
    /// "fresh setup, derive" arms — its other two branches operate on a
    /// profile other than the active one and stay native-only, see this
    /// section's module doc).
    ///
    /// Which branch runs is decided by
    /// `self.panel_app.user_settings_state.needs_vault_unlock` — true only
    /// when a shell has already found an existing, locked vault for this
    /// profile (native: set at boot when an encrypted profile is found
    /// locked). Web does not set this yet — boot-time vault gating for web
    /// is a separate, larger arc — so today this always takes the
    /// fresh-setup branch, which is exactly the path the defect this method
    /// was written for needed a home for: `mlc-app-shell-web` had no
    /// `e2e_setup:` handling at all, so a vault could never be created from
    /// the browser (Settings → "Set Up Encryption").
    pub fn apply_vault_setup(
        &mut self,
        passphrase: &str,
        profile_manager: &mut crate::user_manager::ProfileManager,
    ) -> VaultSetupOutcome {
        if self.panel_app.user_settings_state.needs_vault_unlock {
            // Re-validating an existing vault is exactly what the
            // credentials-modal reseal command already does — reuse it
            // rather than re-deriving/decrypting a second time in this file.
            // `try_unlock_credentials` → `finish_credentials_unlock` already
            // syncs the agent-key store off this same key on success, so
            // this branch does not need its own `sync_agent_key_store` call.
            return match self.try_unlock_credentials(passphrase) {
                Ok(()) => {
                    if let Some(key) = self.core.user().vault_key() {
                        profile_manager.set_vault_key(key);
                    }
                    self.panel_app.user_settings_state.needs_vault_unlock = false;
                    VaultSetupOutcome::Ready { recovery_key: None }
                }
                Err(e) => VaultSetupOutcome::WrongPassphrase(e),
            };
        }

        match profile_manager.derive_and_set_vault_key(passphrase) {
            Ok(key) => {
                self.core.user_mut().set_vault_key(Some(key));
                self.core.templates_mut().set_vault_key(Some(key));
                self.sync_agent_key_store(&key);
                // The whole point of this call: a vault key now exists but
                // `vault.enc` itself does not yet — nothing else about this
                // profile changed, so nothing else would mark it dirty.
                // Native gets this "for free" because a fresh setup is
                // always followed by a skeleton-window rebuild, and
                // `ChartApp::new_window` marks the profile dirty as a side
                // effect of restoring the active preset — but that is an
                // accident of native's window-teardown flow, not something
                // to depend on here (web has no rebuild step at all, so
                // without this call `vault.enc` would never reach
                // `localStorage` — the exact defect this method exists to
                // fix, one step further down).
                self.persist_profile();
                VaultSetupOutcome::Ready { recovery_key: profile_manager.pending_recovery_key.clone() }
            }
            Err(e) => VaultSetupOutcome::Error(e),
        }
    }

    /// Recover access to the CURRENTLY ACTIVE profile's vault via its
    /// recovery key, and unlock it live — same "decrypt, then
    /// `unseal_vault`" shape as [`Self::try_unlock_credentials`], just keyed
    /// by a recovery key instead of a passphrase.
    ///
    /// Native's own `recovery_unlock:` arm instead does a two-step dance
    /// (`set_vault_key` plus a shell-owned `profile_manager.
    /// load_vault_secrets` merge into a copy nothing downstream reads)
    /// because on native it is ALWAYS followed by a full skeleton-window
    /// rebuild that re-seeds `core` from `profile_manager.profile` — that
    /// rebuild is what actually makes the recovered secrets live there.
    /// Neither shell's currently-reachable use of this command has that: on
    /// web there is no rebuild step at all, and native's own live case (a
    /// mid-session credentials-modal reseal via recovery key) is not
    /// followed by one either. Decrypting `vault.enc` here and calling
    /// `unseal_vault` directly makes the secrets live unconditionally, with
    /// no dependence on a rebuild that may not happen.
    ///
    /// A recovered vault always requires a new passphrase before it is
    /// considered settled — on success the caller must route to
    /// `ProfileManagerPage::SetNewPassphrase` (standalone modal) or
    /// `CredentialsView::SetNewPassphrase` (Vault tab), by the same
    /// `show_profile_manager` test as [`Self::apply_vault_setup`]'s doc.
    pub fn apply_recovery_unlock(
        &mut self,
        recovery_key_text: &str,
        profile_manager: &mut crate::user_manager::ProfileManager,
    ) -> Result<(), String> {
        let dir = mlc_core::active_profile_data_dir();
        let salt_path = dir.join("salt.hex");
        let recovery_enc_path = dir.join("recovery_key.enc");
        let vault_path = dir.join("vault.enc");

        let recovery_key_bytes = mlc_vault::crypto::parse_recovery_key(recovery_key_text)
            .map_err(|e| format!("Invalid recovery key format: {}", e))?;

        // Raw storage read, not `mlc_vault::vault::load_encrypted` — this
        // blob is wrapped with the RECOVERY key (`HKDF_RECOVERY_INFO`), not
        // the vault key, so it needs decrypting via
        // `decrypt_master_key_with_recovery` below, not the vault-key path.
        let recovery_storage_key =
            crate::user_manager::profile_manager::path_key(&recovery_enc_path);
        let blob = mlc_vault::global_storage()
            .read(&recovery_storage_key)
            .map_err(|e| format!("Failed to read recovery data: {}", e))?
            .ok_or_else(|| "No recovery data found for this profile".to_string())?;

        let salt = mlc_vault::crypto::load_or_create_salt(&salt_path)?;
        let master_key = mlc_vault::crypto::decrypt_master_key_with_recovery(
            &blob,
            &recovery_key_bytes,
            &salt,
        )
        .map_err(|_| "Wrong recovery key — decryption failed".to_string())?;
        let vault_key = mlc_vault::crypto::derive_vault_key(&master_key, &salt);

        let secrets = mlc_vault::vault::load_encrypted::<mlc_core::user_profile::VaultSecrets>(
            &vault_key,
            &vault_path,
        )
        .map_err(|e| format!("Recovered key did not decrypt the vault: {}", e))?;

        self.core.user_mut().unseal_vault(vault_key, secrets);
        self.sync_agent_key_store(&vault_key);
        profile_manager.set_vault_key(vault_key);
        self.panel_app.user_settings_state.needs_vault_unlock = false;
        Ok(())
    }

    /// Re-key the CURRENTLY ACTIVE profile's vault with a new passphrase —
    /// the mandatory step after a recovery-key unlock. Delegates the actual
    /// re-encryption to
    /// [`crate::user_manager::ProfileManager::rekey_vault`] and syncs the
    /// new key into `core` (the "Phase D fix" this codebase's own history
    /// already had to learn once for the passphrase-validation path:
    /// forgetting `core.templates_mut()` here leaves encrypted template
    /// saves using a stale key until something unrelated happens to reset
    /// it).
    ///
    /// Returns the new formatted recovery key on success — the caller must
    /// show it once (`ProfileManagerPage::ShowRecoveryKey` or
    /// `CredentialsView::ShowRecoveryKey`, same host test as above).
    ///
    /// Also re-encrypts the `agent_keys.enc` sidecar under the new
    /// `VaultKey` (piece 3 of the agent-api-auth-and-secret-store plan left
    /// this as a documented gap — closed here, in piece 4): `rekey_vault`
    /// re-encrypts `vault.enc` synchronously, but without this, any agent
    /// key minted before the re-key would survive on disk yet go unreadable
    /// by `agent_key_store` (and by the render thread's own load-append-save
    /// mint/revoke path) until the next full unlock happened to re-sync it.
    pub fn apply_new_passphrase(
        &mut self,
        new_passphrase: &str,
        profile_manager: &mut crate::user_manager::ProfileManager,
    ) -> Result<String, String> {
        let old_key = profile_manager.vault_key;
        let formatted = profile_manager.rekey_vault(new_passphrase)?;
        if let Some(new_key) = profile_manager.vault_key {
            self.core.user_mut().set_vault_key(Some(new_key));
            self.core.templates_mut().set_vault_key(Some(new_key));
            self.rekey_agent_key_store(old_key, &new_key);
            // `rekey_vault` already re-encrypted `vault.enc` synchronously
            // under the new key, but `templates/*` on disk are still under
            // the OLD one — both shells' persistence drains re-save
            // templates unconditionally whenever the profile is dirty for
            // ANY reason (`save_persistent_state`'s step 6 on web; native's
            // own `templates_disk_dirty` debounce-bypass covers the same
            // ground natively), so marking dirty here is what actually
            // makes that re-encrypt happen promptly instead of waiting on
            // an unrelated future edit.
            self.persist_profile();
        }
        Ok(formatted)
    }

    /// Re-encrypt the `agent_keys.enc` sidecar from `old_key` to `new_key`
    /// after [`Self::apply_new_passphrase`] re-keys the main vault, then
    /// re-syncs the wired [`Self::agent_key_store`] off the new key so a
    /// live session keeps working with its already-issued keys instead of
    /// finding them unreadable until the next full unlock. `old_key` is
    /// `None` only if the vault was never unlocked to begin with, in which
    /// case `rekey_vault` itself would already have refused — kept as a
    /// guard rather than an `expect`, not a path this is expected to take.
    ///
    /// A profile that never minted a key has no sidecar file at all; that
    /// read failure (not just a wrong key) is treated the same as "nothing
    /// to carry forward" — the main vault's own re-key already succeeded by
    /// the time this runs, and a missing sidecar is not a reason to undo it.
    fn rekey_agent_key_store(
        &self,
        old_key: Option<mlc_vault::vault::VaultKey>,
        new_key: &mlc_vault::vault::VaultKey,
    ) {
        let Some(old_key) = old_key else { return };
        let vault_path = mlc_core::active_profile_data_dir().join("agent_keys.enc");
        let records = match mlc_vault::vault::load_encrypted::<serde_json::Value>(&old_key, &vault_path) {
            Ok(records) => records,
            Err(e) => {
                eprintln!("[ChartApp] agent key store: agent_keys.enc not re-keyed ({}) — likely no keys minted yet", e);
                return;
            }
        };
        if let Err(e) = mlc_vault::vault::save_encrypted(new_key, &vault_path, &records) {
            eprintln!("[ChartApp] agent key store: failed to re-key agent_keys.enc: {}", e);
            return;
        }
        self.sync_agent_key_store(new_key);
    }
}

/// Which "add key" form field — used by both the click handler and the Tab
/// chain in `text_input::dispatch`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CredAddField {
    Exchange,
    Label,
    ApiKey,
    ApiSecret,
    Passphrase,
}

impl CredAddField {
    /// Next field in Tab order, wrapping back to the first.
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Exchange => Self::Label,
            Self::Label => Self::ApiKey,
            Self::ApiKey => Self::ApiSecret,
            Self::ApiSecret => Self::Passphrase,
            Self::Passphrase => Self::Exchange,
        }
    }
}
