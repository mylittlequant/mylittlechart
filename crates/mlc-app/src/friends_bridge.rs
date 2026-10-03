//! Friends wire models -> Friends tab state.
//!
//! The transport hands back `mlc_cloud` friend models; the tab holds plain
//! lists and a [`FriendNote`]. This is the one place the two meet, so every
//! shell maps an answer the same way and the mapping is tested without a
//! browser. A missing account and a hidden one are the same `404`, and they
//! stay one [`FriendNote::NoSuchUser`] here.

use mlc_app_layout::ui::friends::{FriendInviteRow, FriendNote};
use mlc_app_layout::ui::modal_settings::UserSettingsState;
use mlc_cloud::{FriendAccepted, FriendError, FriendInvite, FriendRoster, FRIEND_ACTIONS};

/// What the tab's `friend_cmd` string asks the shell to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FriendCmd {
    /// Re-read the roster and the invite links.
    Load,
    /// Send the toggle's current value.
    Discoverable,
    /// Create a personal invite link.
    CreateInvite,
    /// Redeem the redeem box's link or code.
    Accept,
    /// Revoke one invite link.
    RevokeInvite(String),
    /// One of [`FRIEND_ACTIONS`] on a nick.
    Act { action: String, nick: String },
}

/// Read a `friend_cmd` string. Anything unknown is a plain reload.
pub fn parse_cmd(cmd: &str) -> FriendCmd {
    match cmd {
        "load" => return FriendCmd::Load,
        "discoverable" => return FriendCmd::Discoverable,
        "invite" => return FriendCmd::CreateInvite,
        "accept" => return FriendCmd::Accept,
        _ => {}
    }
    match cmd.split_once(':') {
        Some(("irevoke", token)) if !token.is_empty() => FriendCmd::RevokeInvite(token.to_string()),
        Some((action, nick)) if !nick.is_empty() && FRIEND_ACTIONS.contains(&action) => {
            FriendCmd::Act { action: action.to_string(), nick: nick.to_string() }
        }
        _ => FriendCmd::Load,
    }
}

/// The status line for a refused call.
pub fn note_for_error(e: FriendError) -> FriendNote {
    match e {
        FriendError::NoSuchUser => FriendNote::NoSuchUser,
        FriendError::IsYou => FriendNote::IsYou,
        FriendError::NoRequest => FriendNote::NoRequest,
        FriendError::Blocked => FriendNote::Blocked,
        FriendError::RateLimited => FriendNote::RateLimited,
        FriendError::InviteInvalid => FriendNote::InviteInvalid,
        FriendError::SignedOut | FriendError::Network | FriendError::Other => FriendNote::Generic,
    }
}

/// The status line after an invite was redeemed.
pub fn accepted_note(a: &FriendAccepted) -> FriendNote {
    if a.accepted {
        FriendNote::Accepted { nick: a.friend.clone().filter(|n| !n.is_empty()) }
    } else {
        FriendNote::InviteInvalid
    }
}

/// Put a roster into the tab. The status line is left alone.
pub fn apply_roster(st: &mut UserSettingsState, roster: FriendRoster) {
    st.discoverable = roster.discoverable;
    st.friend_nicks = roster.friends;
    st.friend_incoming = roster.incoming;
    st.friend_outgoing = roster.outgoing;
    st.friend_subscriptions = roster.subscriptions;
    st.friend_subscribers = roster.subscribers;
    st.friend_blocked = roster.blocked;
}

/// Put the invite-link list into the tab.
pub fn apply_invites(st: &mut UserSettingsState, invites: Vec<FriendInvite>) {
    st.friend_invites = invites
        .into_iter()
        .map(|i| FriendInviteRow { token: i.token, uses: i.uses, max_uses: i.max_uses, expires_at: i.expires_at })
        .collect();
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlc_app_layout::ui::friends::cmd_for_hit;

    #[test]
    fn every_command_string_parses_to_its_command() {
        assert_eq!(parse_cmd("load"), FriendCmd::Load);
        assert_eq!(parse_cmd("discoverable"), FriendCmd::Discoverable);
        assert_eq!(parse_cmd("invite"), FriendCmd::CreateInvite);
        assert_eq!(parse_cmd("accept"), FriendCmd::Accept);
        assert_eq!(parse_cmd("irevoke:tok_1"), FriendCmd::RevokeInvite("tok_1".into()));
        assert_eq!(
            parse_cmd("decline:bob"),
            FriendCmd::Act { action: "decline".into(), nick: "bob".into() }
        );
        for unknown in ["", "nonsense", "request:", "explode:bob", "irevoke:"] {
            assert_eq!(parse_cmd(unknown), FriendCmd::Load, "{unknown:?}");
        }
    }

    #[test]
    fn every_button_the_tab_draws_reaches_an_action_the_server_takes() {
        for hit in [
            "friend_add:n",
            "friend_sub:n",
            "friend_approve:n",
            "friend_decline:n",
            "friend_remove:n",
            "friend_block:n",
            "friend_unblock:n",
            "friend_unsub:n",
        ] {
            let cmd = cmd_for_hit(hit).unwrap_or_default();
            assert!(matches!(parse_cmd(&cmd), FriendCmd::Act { .. }), "{hit} -> {cmd}");
        }
        let revoke = cmd_for_hit("friend_irevoke:tok").unwrap_or_default();
        assert_eq!(parse_cmd(&revoke), FriendCmd::RevokeInvite("tok".into()));
    }

    #[test]
    fn a_hidden_nick_and_a_missing_one_are_one_note() {
        let hidden = mlc_cloud::friend_error(&mlc_cloud::CloudError::Http {
            status: 404,
            body: r#"{"error":"no such nick"}"#.into(),
        });
        let missing = mlc_cloud::friend_error(&mlc_cloud::CloudError::Http { status: 404, body: String::new() });
        assert_eq!(note_for_error(hidden), note_for_error(missing));
        assert_eq!(note_for_error(hidden), FriendNote::NoSuchUser);
    }

    #[test]
    fn every_refusal_has_its_own_sentence() {
        assert_eq!(note_for_error(FriendError::IsYou), FriendNote::IsYou);
        assert_eq!(note_for_error(FriendError::NoRequest), FriendNote::NoRequest);
        assert_eq!(note_for_error(FriendError::Blocked), FriendNote::Blocked);
        assert_eq!(note_for_error(FriendError::RateLimited), FriendNote::RateLimited);
        assert_eq!(note_for_error(FriendError::InviteInvalid), FriendNote::InviteInvalid);
        for other in [FriendError::Network, FriendError::Other, FriendError::SignedOut] {
            assert_eq!(note_for_error(other), FriendNote::Generic);
        }
    }

    #[test]
    fn an_accepted_invite_names_the_friend_and_a_refused_one_is_invalid() {
        let named = FriendAccepted { accepted: true, friend: Some("nix".into()) };
        assert_eq!(accepted_note(&named), FriendNote::Accepted { nick: Some("nix".into()) });
        let nameless = FriendAccepted { accepted: true, friend: None };
        assert_eq!(accepted_note(&nameless), FriendNote::Accepted { nick: None });
        let refused = FriendAccepted { accepted: false, friend: None };
        assert_eq!(accepted_note(&refused), FriendNote::InviteInvalid);
    }

    #[test]
    fn a_roster_fills_every_list_and_the_flag_and_an_invite_list_keeps_its_rows() {
        let mut st = UserSettingsState::default();
        assert!(!st.discoverable, "hidden by default");
        apply_roster(
            &mut st,
            FriendRoster {
                discoverable: true,
                friends: vec!["a".into()],
                incoming: vec!["b".into()],
                outgoing: vec!["c".into()],
                subscriptions: vec!["d".into()],
                subscribers: vec!["e".into()],
                blocked: vec!["f".into()],
            },
        );
        assert!(st.discoverable);
        assert_eq!(
            (
                st.friend_nicks.len(),
                st.friend_incoming.len(),
                st.friend_outgoing.len(),
                st.friend_subscriptions.len(),
                st.friend_subscribers.len(),
                st.friend_blocked.len()
            ),
            (1, 1, 1, 1, 1, 1)
        );
        apply_invites(
            &mut st,
            vec![FriendInvite {
                token: "tok".into(),
                max_uses: 3,
                uses: 1,
                created_at: "x".into(),
                expires_at: "2026-10-15T00:00:00+00:00".into(),
            }],
        );
        assert_eq!(st.friend_invites.len(), 1);
        assert_eq!((st.friend_invites[0].uses, st.friend_invites[0].max_uses), (1, 3));
    }
}
