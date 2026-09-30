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
//! design — this app now constructs TWO of them, `agent_key_store` and
//! `alert_secrets_store` (see [`ChartApp::vault_key_sidecars`]), but the
//! Vault tab's own status row still does NOT drive off either: swapping
//! `BackendCore::UserProfileManager::vault_key` for a `SecretStore` is its
//! own piece of work, out of scope here. This tab drives off the vault key
//! that IS already wired end to end: `self.core.user().vault_key()` (the
//! same `Option<VaultKey>` `AlertSettingsState::secrets_available` already
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
                log::warn!("[ChartApp] credentials unhandled: {}", rest);
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
        self.panel_app.user_settings_state.open();
    }

    /// Open the small vault-unlock overlay drawn on top of the Community
    /// modal (owner ruling 2026-09-10, DM pane item 3) — replaces
    /// `"cloud_overlay:dm:unlock_vault"` opening the WHOLE User Settings
    /// modal via [`Self::open_vault_tab`], which then rendered UNDER the
    /// community modal (both `ZLayer::Modal`; render-call order decided
    /// which one painted last — see `chart_panel_app.rs::render_modals`'s
    /// own doc for the general fix). This overlay draws at
    /// `ZLayer::ModalOverlay` instead, unconditionally above every ordinary
    /// modal, so it needs no part in that fix.
    ///
    /// Reuses the SAME shared vault page
    /// (`layout::modals::profile_manager::render_page_unlock`,
    /// `VaultPageHost::Overlay`) and the SAME `cred_unlock:{passphrase}`
    /// shell command the Vault tab's own "Unlock" button fires
    /// (`Self::try_unlock_credentials`) — never `e2e_setup:`, since DM is
    /// only ever reachable for a paid account that already has a vault to
    /// unlock. `pending_unlock_return` stays untouched here (explicitly
    /// cleared): unlike the Vault tab's own routes, success must NOT pop
    /// open User Settings — see [`Self::finish_credentials_unlock`]'s own
    /// `show_dm_vault_unlock_overlay` branch.
    #[cfg(feature = "cloud")]
    pub(super) fn open_dm_vault_unlock_overlay(&mut self) {
        self.panel_app.credentials_state.pending_unlock_return = None;
        self.panel_app.user_settings_state.vault_unlock_error = None;
        self.panel_app.user_settings_state.vault_unlock_attempts = 0;
        self.panel_app.user_settings_state.passphrase_revealed = false;
        self.panel_app.user_settings_state.show_dm_vault_unlock_overlay = true;
        let mut ic = self.input_coordinator.borrow_mut();
        let tf = ic.text_fields_mut();
        let wid = uzor::WidgetId::from(text_input::NEW_PASSPHRASE);
        tf.clear(&wid);
        tf.focus(wid);
    }

    /// Close the DM vault-unlock overlay without unlocking — Cancel, Esc,
    /// the × or a dimmer click all land here (`panel_click.rs`'s
    /// `"dm_vault_unlock:"` chrome arm).
    #[cfg(feature = "cloud")]
    pub(super) fn close_dm_vault_unlock_overlay(&mut self) {
        self.panel_app.user_settings_state.show_dm_vault_unlock_overlay = false;
        self.panel_app.user_settings_state.vault_unlock_error = None;
        let mut ic = self.input_coordinator.borrow_mut();
        let tf = ic.text_fields_mut();
        tf.clear(&uzor::WidgetId::from(text_input::NEW_PASSPHRASE));
        tf.blur();
    }

    /// True while the Vault tab (User Settings modal) is open and showing
    /// one of the six shared vault pages that also render under the
    /// standalone Profile Manager modal / welcome wizard host
    /// (`VaultPageHost::{Tab,Modal}` — `layout::modals::profile_manager`'s
    /// module doc): `UnlockPassphrase`, `CreatePassphrase`,
    /// `ShowRecoveryKey`, `ConfirmRecoveryKey`, `UseRecoveryKey`,
    /// `SetNewPassphrase`.
    ///
    /// Exists so `mlc-app/src/input/mod.rs`'s drag pre-dispatch block (full
    /// hit-test over `input_char_positions`, exclusive focus-flag clearing
    /// across every sibling field) can gate on ONE predicate that is true
    /// under either host, instead of only ever being true for the
    /// standalone modal (`show_profile_manager`/`show_welcome_wizard`).
    /// Without this the Tab host falls through to the incomplete per-id
    /// click arms in `panel_click.rs`, which clear fewer sibling flags and
    /// never arm drag-select — the root cause of two focus rings at once
    /// and no drag-selection on the Vault tab's passphrase/recovery-key
    /// fields.
    pub(crate) fn vault_pages_active(&self) -> bool {
        self.panel_app.user_settings_state.is_open
            && self.panel_app.user_settings_state.active_tab == UserSettingsTab::Vault
            && matches!(
                self.panel_app.credentials_state.view,
                CredentialsView::UnlockPassphrase
                    | CredentialsView::CreatePassphrase
                    | CredentialsView::ShowRecoveryKey
                    | CredentialsView::ConfirmRecoveryKey
                    | CredentialsView::UseRecoveryKey
                    | CredentialsView::SetNewPassphrase
            )
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

    /// Every RAM-only vault-keyed `SecretStore` sidecar this window's
    /// `ChartApp` holds — the agent-key store and the alert-secrets store —
    /// one list [`Self::seal_vault_sidecars`] and [`Self::sync_vault_sidecars`]
    /// both iterate, so a THIRD sidecar (should one ever join these two)
    /// needs one new entry here rather than a new copy of every call-out
    /// below. `None` entries (a shell that never wired a given store) are
    /// filtered out by the callers, not here.
    fn vault_key_sidecars(&self) -> [Option<&std::sync::Arc<mlc_vault::SecretStore>>; 2] {
        [self.agent_key_store.as_ref(), self.alert_secrets_store.as_ref()]
    }

    /// Seal every store in [`Self::vault_key_sidecars`] — the counterpart of
    /// [`Self::sync_vault_sidecars`], called from every path that seals the
    /// main vault so no sidecar outlives it.
    fn seal_vault_sidecars(&self) {
        for store in self.vault_key_sidecars().into_iter().flatten() {
            store.seal();
        }
    }

    /// Refresh every store in [`Self::vault_key_sidecars`] off the SAME
    /// `VaultKey` that just unlocked the main vault — called from every path
    /// that derives or restores one (`finish_credentials_unlock`,
    /// `apply_vault_setup`'s fresh-setup branch, `apply_recovery_unlock`).
    /// `unlock_with_key` skips PBKDF2 entirely (the render thread already
    /// holds the derived key here, never a passphrase to re-derive from).
    fn sync_vault_sidecars(&self, vault_key: &mlc_vault::vault::VaultKey) {
        for store in self.vault_key_sidecars().into_iter().flatten() {
            if let Err(e) = store.unlock_with_key(vault_key) {
                log::warn!("[ChartApp] secret store: failed to unlock alongside vault: {}", e);
            }
        }
    }

    /// One-time migration seed (alerts-account-wide-and-telegram-delivery
    /// plan §3): a profile unlocked before `alert_secrets.enc` existed still
    /// carries its Telegram bot token, subscriber list and webhook URL
    /// inside `VaultSecrets` (decrypted into `profile.domain.
    /// notification_secrets` by `unseal_vault` just above). Copies whatever
    /// is there into the sidecar and clears the field from `VaultSecrets` —
    /// idempotent by construction: once cleared, the next unlock finds
    /// nothing to migrate, so this never fires twice for the same profile.
    /// Call right after [`Self::sync_vault_sidecars`] has unlocked the
    /// sidecar off the same key.
    ///
    /// Boot-time auto-unlock needs no separate call site: `ProfileManager::
    /// load` only ever receives an already-derived `VaultKey` when an
    /// earlier call-out in THIS same run already unlocked (and therefore
    /// already migrated) it — a cold boot always starts sealed and reaches
    /// this via [`Self::finish_credentials_unlock`] or
    /// [`Self::apply_recovery_unlock`], the two paths that actually decrypt
    /// a fresh `VaultSecrets` from disk.
    fn migrate_notification_secrets_into_sidecar(&mut self, vault_key: &mlc_vault::vault::VaultKey) {
        if self.alert_secrets_store.is_none() {
            return;
        }
        let legacy = self.core.user().profile().domain.notification_secrets.clone();
        let has_secret = !legacy.telegram.bot_token.is_empty()
            || !legacy.telegram.subscribers.is_empty()
            || !legacy.webhook.url.is_empty();
        if !has_secret {
            return;
        }
        let alert_secrets_path = mlc_core::active_profile_data_dir().join("alert_secrets.enc");
        if let Err(e) = mlc_vault::vault::save_encrypted(vault_key, &alert_secrets_path, &legacy) {
            log::warn!("[ChartApp] alert secrets: migration write failed, leaving the token in vault.enc: {}", e);
            return;
        }
        self.core.user_mut().profile_mut().domain.notification_secrets = Default::default();
        self.persist_profile();
        // Re-read the freshly-written payload into the sidecar's RAM cache —
        // it was unlocked (off the same key) before this write landed.
        self.sync_vault_sidecars(vault_key);
        log::debug!("[ChartApp] alert secrets: migrated Telegram/webhook secrets from vault.enc into alert_secrets.enc (one-time)");
    }

    /// Seal the vault deliberately (the "Lock" button) — the counterpart a
    /// user must have alongside "Unlock" per the owner's ask, since until
    /// now there was only ever unlock.
    fn lock_vault(&mut self) {
        self.core.user_mut().seal_vault();
        self.seal_vault_sidecars();

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
        log::debug!("[ChartApp] credentials: vault sealed");
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
        log::debug!(
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
        log::debug!("[ChartApp] credentials: exchange key added");
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
        self.sync_vault_sidecars(&key);
        self.migrate_notification_secrets_into_sidecar(&key);
        self.refresh_alert_secrets_mirror();

        self.sync_credentials_from_core();

        self.panel_app.user_settings_state.vault_unlock_error = None;

        // DM's own mini overlay (owner ruling 2026-09-10, item 3) closes in
        // place on success — its caller never asked for the full User
        // Settings modal, so nothing here may pop it open. The open DM
        // conversation redecrypts itself on the very next frame: both
        // shells' own per-frame DM driver re-reads `core.user().vault_key()`
        // unconditionally (`cloud_session.rs`/`main.rs`'s own "Messages tab"
        // block, "runs regardless of `messages_active`/tab") and calls
        // `redecrypt_open_conversation` the moment it sees a key.
        if self.panel_app.user_settings_state.show_dm_vault_unlock_overlay {
            self.panel_app.user_settings_state.show_dm_vault_unlock_overlay = false;
            log::debug!("[ChartApp] credentials: vault unlocked mid-session (DM overlay)");
            return;
        }

        self.close_profile_manager();

        if let Some(view) = self.panel_app.credentials_state.pending_unlock_return.take() {
            self.panel_app.user_settings_state.active_tab = UserSettingsTab::Vault;
            self.panel_app.user_settings_state.open();
            self.panel_app.credentials_state.view = view;
        }
        log::debug!("[ChartApp] credentials: vault unlocked mid-session");
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
        self.close_profile_manager();
        self.panel_app.user_settings_state.vault_unlock_error = None;
        self.sync_credentials_from_core();

        if let Some(view) = self.panel_app.credentials_state.pending_unlock_return.take() {
            self.panel_app.user_settings_state.active_tab = UserSettingsTab::Vault;
            self.panel_app.user_settings_state.open();
            self.panel_app.credentials_state.view = view;
            log::debug!("[ChartApp] credentials: vault created, returning to {:?}", view);
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
            // syncs every vault-key sidecar off this same key on success, so
            // this branch does not need its own `sync_vault_sidecars` call.
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
                self.sync_vault_sidecars(&key);
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
        self.sync_vault_sidecars(&vault_key);
        self.migrate_notification_secrets_into_sidecar(&vault_key);
        self.refresh_alert_secrets_mirror();
        profile_manager.set_vault_key(vault_key);
        self.panel_app.user_settings_state.needs_vault_unlock = false;
        Ok(())
    }

    /// Open the CURRENTLY ACTIVE profile's vault with the vault key `V` of a
    /// key account (anonymous purchase or invite — see
    /// `mlc_vault::account_key`), right after a sign-in by key.
    ///
    /// There is no passphrase and no prompt: `V` is derived on this device from
    /// the account key the person just typed or was shown, and it is the vault
    /// key. An existing `vault.enc` must open under it (then it is unsealed
    /// live, the same steps as [`Self::apply_recovery_unlock`]); with none, `V`
    /// becomes the profile's vault key and the profile is marked dirty so
    /// `vault.enc` is written under it. No recovery key is made — the account
    /// key is the recovery.
    ///
    /// A profile already sealed with a different key (a local passphrase vault
    /// from before the account key existed) is left sealed and reported as an
    /// error: a key account never overwrites a vault it cannot open.
    pub fn apply_account_vault_key(
        &mut self,
        vault_key: mlc_vault::vault::VaultKey,
        profile_manager: &mut crate::user_manager::ProfileManager,
    ) -> Result<(), String> {
        let vault_path = mlc_core::active_profile_data_dir().join("vault.enc");
        let storage_key = crate::user_manager::profile_manager::path_key(&vault_path);
        if mlc_vault::global_storage().exists(&storage_key) {
            let secrets = mlc_vault::vault::load_encrypted::<mlc_core::user_profile::VaultSecrets>(
                &vault_key,
                &vault_path,
            )
            .map_err(|_| "This profile's vault is sealed with a different key".to_string())?;
            self.core.user_mut().unseal_vault(vault_key, secrets);
            self.sync_vault_sidecars(&vault_key);
            self.migrate_notification_secrets_into_sidecar(&vault_key);
            self.refresh_alert_secrets_mirror();
            profile_manager.set_vault_key(vault_key);
            self.panel_app.user_settings_state.needs_vault_unlock = false;
            return Ok(());
        }
        profile_manager.set_vault_key(vault_key);
        self.core.user_mut().set_vault_key(Some(vault_key));
        self.core.templates_mut().set_vault_key(Some(vault_key));
        self.sync_vault_sidecars(&vault_key);
        // A vault key now exists but `vault.enc` does not yet; nothing else
        // changed, so nothing else would mark the profile dirty.
        self.persist_profile();
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
    /// Also re-encrypts every sidecar in [`Self::vault_key_sidecars`]
    /// (`agent_keys.enc`, `alert_secrets.enc`) under the new `VaultKey`
    /// (piece 3 of the agent-api-auth-and-secret-store plan left the
    /// agent-key half of this as a documented gap — closed in piece 4,
    /// extended to the alert-secrets sidecar here): `rekey_vault`
    /// re-encrypts `vault.enc` synchronously, but without this, any secret
    /// written to a sidecar before the re-key would survive on disk yet go
    /// unreadable by that sidecar's `SecretStore` until the next full
    /// unlock happened to re-sync it.
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
            self.rekey_secret_sidecars(old_key, &new_key);
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

    /// Re-encrypt every sidecar in [`Self::vault_key_sidecars`] from
    /// `old_key` to `new_key`, one filename per store, after
    /// [`Self::apply_new_passphrase`] re-keys the main vault, then re-syncs
    /// each store off the new key so a live session keeps working with its
    /// already-written contents instead of finding them unreadable until
    /// the next full unlock. `old_key` is `None` only if the vault was
    /// never unlocked to begin with, in which case `rekey_vault` itself
    /// would already have refused — kept as a guard rather than an
    /// `expect`, not a path this is expected to take.
    ///
    /// A store that never wrote anything has no sidecar file at all; that
    /// read failure (not just a wrong key) is treated the same as "nothing
    /// to carry forward for THIS store" and does not stop the others — the
    /// main vault's own re-key already succeeded by the time this runs, and
    /// a missing sidecar is not a reason to undo it.
    fn rekey_secret_sidecars(
        &self,
        old_key: Option<mlc_vault::vault::VaultKey>,
        new_key: &mlc_vault::vault::VaultKey,
    ) {
        let Some(old_key) = old_key else { return };
        let dir = mlc_core::active_profile_data_dir();
        for (filename, store) in [
            ("agent_keys.enc", self.agent_key_store.as_ref()),
            ("alert_secrets.enc", self.alert_secrets_store.as_ref()),
        ] {
            let Some(store) = store else { continue };
            let vault_path = dir.join(filename);
            let records = match mlc_vault::vault::load_encrypted::<serde_json::Value>(&old_key, &vault_path) {
                Ok(records) => records,
                Err(e) => {
                    log::debug!("[ChartApp] secret store: {filename} not re-keyed ({}) — likely nothing stored yet", e);
                    continue;
                }
            };
            if let Err(e) = mlc_vault::vault::save_encrypted(new_key, &vault_path, &records) {
                log::warn!("[ChartApp] secret store: failed to re-key {filename}: {}", e);
                continue;
            }
            if let Err(e) = store.unlock_with_key(new_key) {
                log::warn!("[ChartApp] secret store: {filename} failed to unlock after re-key: {}", e);
            }
        }
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
