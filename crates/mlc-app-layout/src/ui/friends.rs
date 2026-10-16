//! State shapes of the Friends tab and the Account tab's "Show me in search"
//! toggle: the status line and the personal invite-link rows.
//!
//! Accounts are hidden from search by default. A hidden account is added only
//! through a personal invite link or code; a public one takes a request, and
//! the requester is its subscriber until it approves. Every search that finds
//! nothing reads [`FriendsKey::NoSuchUser`], whether the nick does not exist or
//! is hidden: nothing here can tell them apart.

use mlc_core::i18n::{FriendsKey, Language};

use crate::ui::checkout::fill;

/// What the Friends tab's status line says, kept as data and worded at draw
/// time so a language switch re-words it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FriendNote {
    /// A link went to the clipboard.
    Copied,
    /// A friend request went out.
    Requested,
    /// An invite link was opened while signed out.
    SignInToAccept,
    /// An invite was accepted; the inviter's nick when the server named one.
    Accepted { nick: Option<String> },
    NoSuchUser,
    IsYou,
    NoRequest,
    Blocked,
    RateLimited,
    InviteInvalid,
    Generic,
}

impl FriendNote {
    /// The sentence in `lang`.
    pub fn text(&self, lang: Language) -> String {
        match self {
            Self::Copied => FriendsKey::NoteCopied.get(lang).to_string(),
            Self::Requested => FriendsKey::NoteRequested.get(lang).to_string(),
            Self::SignInToAccept => FriendsKey::NoteSignInToAccept.get(lang).to_string(),
            Self::Accepted { nick: Some(nick) } => fill(FriendsKey::NoteAccepted.get(lang), &[("nick", nick)]),
            Self::Accepted { nick: None } => FriendsKey::NoteAcceptedAnon.get(lang).to_string(),
            Self::NoSuchUser => FriendsKey::NoSuchUser.get(lang).to_string(),
            Self::IsYou => FriendsKey::ErrIsYou.get(lang).to_string(),
            Self::NoRequest => FriendsKey::ErrNoRequest.get(lang).to_string(),
            Self::Blocked => FriendsKey::ErrBlocked.get(lang).to_string(),
            Self::RateLimited => FriendsKey::ErrRateLimited.get(lang).to_string(),
            Self::InviteInvalid => FriendsKey::ErrInviteInvalid.get(lang).to_string(),
            Self::Generic => FriendsKey::ErrGeneric.get(lang).to_string(),
        }
    }

    /// Whether the line reads as a problem (danger colour) or a result.
    pub fn is_error(&self) -> bool {
        !matches!(self, Self::Copied | Self::Requested | Self::Accepted { .. } | Self::SignInToAccept)
    }
}

/// One outstanding personal invite link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FriendInviteRow {
    pub token: String,
    pub uses: i64,
    pub max_uses: i64,
    /// RFC 3339.
    pub expires_at: String,
}

impl FriendInviteRow {
    /// The link's shape on the row: the start of the token, never the whole
    /// secret (Copy puts the whole link on the clipboard).
    pub fn label(&self) -> String {
        let head: String = self.token.chars().take(8).collect();
        if self.token.chars().count() > 8 {
            format!("{head}\u{2026}")
        } else {
            head
        }
    }

    /// "1/3 used, expires 2026-10-15".
    pub fn meta(&self, lang: Language) -> String {
        let date: String = self.expires_at.chars().take(10).collect();
        fill(
            FriendsKey::InviteMeta.get(lang),
            &[("uses", &self.uses.to_string()), ("max", &self.max_uses.to_string()), ("date", &date)],
        )
    }
}

/// Hit ids the Friends tab and the Account tab toggle register (after the
/// `user_settings:` prefix). One definition for the renderer and the click
/// router.
pub mod hit {
    /// The Account tab's "Show me in search" toggle.
    pub const DISCOVERABLE: &str = "discoverable";
    /// "New invite link".
    pub const INVITE_CREATE: &str = "friend_invite";
    /// The redeem box's Accept button.
    pub const INVITE_ACCEPT: &str = "friend_accept";
    /// The redeem box (click to focus).
    pub const INVITE_CODE: &str = "friend_code";
    /// The nick search box (click to focus).
    pub const NICK: &str = "friend_nick";
    /// Prefix of an invite row's Copy button; the token follows.
    pub const INVITE_COPY: &str = "friend_icopy:";
    /// Prefix of an invite row's Revoke button; the token follows.
    pub const INVITE_REVOKE: &str = "friend_irevoke:";
}

/// The origin invite links are shared under.
///
/// `MLC_ORIGIN` from `build.local.env` when that value is non-empty.
/// Otherwise the stand-in host.
pub const INVITE_ORIGIN: &str = match option_env!("MLC_ORIGIN") {
    Some(origin) => {
        if origin.is_empty() {
            "https://chart.example"
        } else {
            origin
        }
    }
    None => "https://chart.example",
};

/// The shareable link of an invite token.
pub fn invite_link(token: &str) -> String {
    format!("{INVITE_ORIGIN}/?friend={token}")
}

/// What a Friends-tab button click asks the shell to do, as the `friend_cmd`
/// string the shell drains: `hit` is the widget id after `user_settings:`.
/// `None` for anything that is not one of the tab's per-row buttons (the
/// link Copy button is handled by the app itself, it needs the clipboard).
pub fn cmd_for_hit(hit: &str) -> Option<String> {
    const ROW_ACTIONS: [(&str, &str); 9] = [
        ("friend_add:", "request"),
        ("friend_sub:", "subscribe"),
        ("friend_approve:", "approve"),
        ("friend_decline:", "decline"),
        ("friend_remove:", "remove"),
        ("friend_block:", "block"),
        ("friend_unblock:", "unblock"),
        ("friend_unsub:", "unsubscribe"),
        ("friend_irevoke:", "irevoke"),
    ];
    ROW_ACTIONS.iter().find_map(|(prefix, action)| {
        let key = hit.strip_prefix(prefix)?.trim();
        (!key.is_empty()).then(|| format!("{action}:{key}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EN: Language = Language::En;

    #[test]
    fn row_buttons_become_shell_commands() {
        for (hit, cmd) in [
            ("friend_add:bob", "request:bob"),
            ("friend_sub:bob", "subscribe:bob"),
            ("friend_approve:bob", "approve:bob"),
            ("friend_decline:bob", "decline:bob"),
            ("friend_remove:bob", "remove:bob"),
            ("friend_block:bob", "block:bob"),
            ("friend_unblock:bob", "unblock:bob"),
            ("friend_unsub:bob", "unsubscribe:bob"),
            ("friend_irevoke:tok_1", "irevoke:tok_1"),
        ] {
            assert_eq!(cmd_for_hit(hit).as_deref(), Some(cmd), "{hit}");
        }
        for not_a_command in ["friend_add:", "friend_add: ", "friend_icopy:tok", "friend_nick", "discoverable", ""] {
            assert_eq!(cmd_for_hit(not_a_command), None, "{not_a_command:?}");
        }
    }

    #[test]
    fn an_invite_link_carries_the_token_under_the_friend_query() {
        assert_eq!(invite_link("tok_1"), "https://chart.example/?friend=tok_1");
    }

    #[test]
    fn every_note_reads_in_every_language() {
        let notes = [
            FriendNote::Copied,
            FriendNote::Requested,
            FriendNote::SignInToAccept,
            FriendNote::Accepted { nick: Some("nix".into()) },
            FriendNote::Accepted { nick: None },
            FriendNote::NoSuchUser,
            FriendNote::IsYou,
            FriendNote::NoRequest,
            FriendNote::Blocked,
            FriendNote::RateLimited,
            FriendNote::InviteInvalid,
            FriendNote::Generic,
        ];
        for lang in Language::all() {
            for note in &notes {
                let text = note.text(*lang);
                assert!(!text.is_empty(), "{note:?} in {lang:?}");
                assert!(!text.contains('{'), "unfilled placeholder in {text:?}");
            }
        }
    }

    #[test]
    fn an_accepted_invite_names_the_new_friend() {
        assert_eq!(
            FriendNote::Accepted { nick: Some("nix".into()) }.text(EN),
            "Invite accepted. You are now friends with nix."
        );
        assert_eq!(FriendNote::Accepted { nick: None }.text(EN), "Invite accepted.");
    }

    #[test]
    fn a_hidden_and_a_missing_account_read_the_same() {
        // The client has a single class for both; there is nothing to tell apart.
        assert_eq!(FriendNote::NoSuchUser.text(EN), "No such user.");
        assert_eq!(FriendNote::NoSuchUser.text(Language::Ru), "Такого пользователя нет.");
    }

    #[test]
    fn only_results_are_not_errors() {
        for ok in [FriendNote::Copied, FriendNote::Requested, FriendNote::SignInToAccept] {
            assert!(!ok.is_error(), "{ok:?}");
        }
        assert!(!FriendNote::Accepted { nick: None }.is_error());
        for bad in [FriendNote::NoSuchUser, FriendNote::IsYou, FriendNote::Blocked, FriendNote::Generic] {
            assert!(bad.is_error(), "{bad:?}");
        }
    }

    #[test]
    fn an_invite_row_shows_a_token_stub_and_its_usage() {
        let row = FriendInviteRow {
            token: "abcdefgh12345678".into(),
            uses: 1,
            max_uses: 3,
            expires_at: "2026-10-15T00:00:00+00:00".into(),
        };
        assert_eq!(row.label(), "abcdefgh\u{2026}");
        assert!(!row.label().contains("12345678"), "the whole token never prints on the row");
        assert_eq!(row.meta(EN), "1/3 used, expires 2026-10-15");
        let short = FriendInviteRow { token: "abc".into(), ..row };
        assert_eq!(short.label(), "abc");
    }
}
