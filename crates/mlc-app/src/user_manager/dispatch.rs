//! Shared dispatcher for the five profile-family commands queued into
//! `ChartApp::pending_updater_cmd` by click/text-input handlers
//! (`profile_create`, `profile_rename`, `profile_switch`, `profile_delete`,
//! `profile_set_avatar`).
//!
//! Both shells own a `ProfileManager` at their own layer (the shell's boot/
//! session struct on native, a locally-scoped `run()` binding on web) —
//! `ChartApp` itself never holds one (shared-BackendCore arc: per-window
//! `ChartApp` only reads profile data seeded into `BackendCore`, it does not
//! own multi-profile bookkeeping). [`ChartApp::dispatch_profile_command`] is
//! therefore an associated function, not a `&mut self` method: it takes the
//! caller's `&mut ProfileManager` directly rather than pretending `ChartApp`
//! has one to dispatch through. It still lives in `impl ChartApp` (this
//! file) and is called as `ChartApp::dispatch_profile_command(..)` so both
//! shells reach it through the one type they already share — the
//! `AppHandle { dispatch }` shape named in `CLAUDE.md`'s finish line #2,
//! sized to what actually exists today.
//!
//! Every mutation these five commands make (create/rename/delete/set_avatar
//! a profile) goes through [`ProfileManager`] methods, which route all I/O
//! through `mlc_vault::global_storage()` / `mlc_core::user_profile::storage`
//! — both already storage-backend-agnostic (filesystem on native,
//! `localStorage` on wasm; see `mlc-app-shell-web/src/persistence.rs`'s doc
//! comment, and `mlc_core::user_profile::storage`'s own). No `cfg` gate is
//! needed anywhere in this file.
//!
//! Per the "return an outcome, do not print" contract: this dispatcher never
//! calls `eprintln!`/`console.error` itself. Both the disk-mutation error
//! path and the caller-side follow-up (toast overlay on native, console log
//! on web, `sync_profiles_to_windows`, window fanout, ...) are shell
//! decisions returned to the caller as [`ProfileCommandOutcome`].

use crate::user_manager::ProfileManager;
use crate::ChartApp;

/// What a successful profile-family command actually changed.
///
/// Deliberately data-only — no window/UI fanout lives here, because that
/// fanout differs per shell (native mirrors into every open window's
/// `user_settings_state`; web has exactly one `ChartApp` to update) and
/// was already shell-owned code before this dispatcher existed. Extracting
/// it would have meant duplicating shell-specific multi-window logic into
/// a crate that must not depend on either shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileEffect {
    /// `profile_create:{name}` — a new profile directory + index entry now
    /// exist on disk (not yet active; the caller decides whether/how to
    /// prompt for the new profile's passphrase).
    Created { id: String, display_name: String },
    /// `profile_rename:{id}:{new_name}` — or the legacy single-argument
    /// wire format `profile_rename:{new_name}` (still emitted by
    /// `text_input/dispatch.rs`'s Enter-to-confirm path), which always
    /// targets the active profile. `was_active` tells the caller whether
    /// its own in-memory mirror of the active profile's display name also
    /// needs updating.
    Renamed { id: String, new_name: String, was_active: bool },
    /// `profile_set_avatar:{avatar}` — the wire format carries NO profile
    /// id (its only producer, `panel_click.rs`, always targets the active
    /// profile), so this always reports the active profile's id.
    AvatarSet { id: String, avatar: String },
    /// `profile_delete:{id}` — profile directory + index entry removed.
    Deleted { id: String },
    /// `profile_switch:{id}` names a real, different, existing profile.
    /// The dispatcher only validates (exists, not already active) —
    /// actually tearing down/rebuilding windows or session state is
    /// shell-specific and stays with the caller (native:
    /// `App::execute_profile_switch`; web currently has no multi-profile
    /// session swap at all — see the dispatcher's own doc comment on why
    /// this variant exists without an executor on that side yet).
    SwitchRequested { id: String },
    /// `profile_switch:{id}` named the profile that is already active —
    /// nothing to do.
    SwitchAlreadyActive,
}

/// Result of [`ChartApp::dispatch_profile_command`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileCommandOutcome {
    /// One of the five profile-family commands matched and its
    /// `ProfileManager` call succeeded.
    Handled(ProfileEffect),
    /// One of the five profile-family commands matched but its
    /// `ProfileManager` call failed (I/O error, unknown profile id, ...).
    /// Carries the message for whatever the shell uses as its error
    /// surface (native: toast overlay; web: `console.error`).
    Failed(String),
    /// `cmd` is not one of the five commands this dispatcher owns. The
    /// caller must try its own remaining arms (vault/e2e, `logout`,
    /// `start_device_auth`, ...) or log it as genuinely unhandled — a
    /// command that reaches neither shell's dispatch must never disappear
    /// without a trace (see `mylittlechart/CLAUDE.md`'s wasm shell
    /// contract).
    NotForMe,
}

impl ChartApp {
    /// Dispatch one of the five profile-family commands queued into
    /// `pending_updater_cmd`. See the module doc comment for why this is an
    /// associated function rather than a `&mut self` method.
    pub fn dispatch_profile_command(
        cmd: &str,
        profile_manager: &mut ProfileManager,
    ) -> ProfileCommandOutcome {
        if let Some(rest) = cmd.strip_prefix("profile_rename:") {
            let (target_id, new_name) = match rest.find(':') {
                Some(idx) => (rest[..idx].to_string(), rest[idx + 1..].to_string()),
                None => (
                    profile_manager.profile.domain.profile_id.clone(),
                    rest.to_string(),
                ),
            };
            let was_active = target_id == profile_manager.profile.domain.profile_id;
            return match profile_manager.rename_profile(&target_id, &new_name) {
                Ok(()) => ProfileCommandOutcome::Handled(ProfileEffect::Renamed {
                    id: target_id,
                    new_name,
                    was_active,
                }),
                Err(e) => ProfileCommandOutcome::Failed(e),
            };
        }

        if let Some(avatar) = cmd.strip_prefix("profile_set_avatar:") {
            let active_id = profile_manager.profile.domain.profile_id.clone();
            return match profile_manager.set_avatar(&active_id, avatar) {
                Ok(()) => ProfileCommandOutcome::Handled(ProfileEffect::AvatarSet {
                    id: active_id,
                    avatar: avatar.to_string(),
                }),
                Err(e) => ProfileCommandOutcome::Failed(e),
            };
        }

        if let Some(rest) = cmd.strip_prefix("profile_create:") {
            let name_opt = if rest.trim().is_empty() { None } else { Some(rest) };
            return match profile_manager.create_profile(name_opt, "chart") {
                Ok(meta) => ProfileCommandOutcome::Handled(ProfileEffect::Created {
                    id: meta.id,
                    display_name: meta.display_name,
                }),
                Err(e) => ProfileCommandOutcome::Failed(e),
            };
        }

        if let Some(id) = cmd.strip_prefix("profile_switch:") {
            if id == profile_manager.profile.domain.profile_id.as_str() {
                return ProfileCommandOutcome::Handled(ProfileEffect::SwitchAlreadyActive);
            }
            let exists = profile_manager
                .available_profiles()
                .iter()
                .any(|p| p.id == id);
            return if exists {
                ProfileCommandOutcome::Handled(ProfileEffect::SwitchRequested {
                    id: id.to_string(),
                })
            } else {
                ProfileCommandOutcome::Failed(format!("profile {} not found", id))
            };
        }

        if let Some(id) = cmd.strip_prefix("profile_delete:") {
            return match profile_manager.delete_profile(id) {
                Ok(()) => ProfileCommandOutcome::Handled(ProfileEffect::Deleted {
                    id: id.to_string(),
                }),
                Err(e) => ProfileCommandOutcome::Failed(e),
            };
        }

        ProfileCommandOutcome::NotForMe
    }
}
