//! `/api/account/friends`, `/api/account/friend-invites` and
//! `/api/account/discoverable` — wire shapes and error classification.
//!
//! Accounts are hidden from search by default. A hidden account is reached
//! only through a personal invite link or code; a public one takes a request
//! (the requester is its subscriber until it approves). Every lookup that
//! finds nothing, for a nick that does not exist or one that is hidden, is the
//! same `404` — [`friend_error`] maps it to [`FriendError::NoSuchUser`] and
//! nothing on the client tells the two apart.

use serde::Deserialize;

use crate::CloudError;

/// `GET /api/account/friends`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct FriendRoster {
    /// Whether nick search may find this account. Off until the person turns it on.
    #[serde(default)]
    pub discoverable: bool,
    #[serde(default)]
    pub friends: Vec<String>,
    /// Requests waiting for this account's answer.
    #[serde(default)]
    pub incoming: Vec<String>,
    /// Requests this account sent, not yet approved.
    #[serde(default)]
    pub outgoing: Vec<String>,
    /// Accounts this account follows.
    #[serde(default)]
    pub subscriptions: Vec<String>,
    /// Accounts following this one (a requester is one until approved).
    #[serde(default)]
    pub subscribers: Vec<String>,
    #[serde(default)]
    pub blocked: Vec<String>,
}

/// `POST /api/account/friend-invites`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct FriendInviteCreated {
    pub token: String,
    /// `/?friend=<token>`.
    pub url_path: String,
    #[serde(default)]
    pub max_uses: i64,
    #[serde(default)]
    pub expires_at: String,
}

/// One row of `GET /api/account/friend-invites`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct FriendInvite {
    pub token: String,
    #[serde(default)]
    pub max_uses: i64,
    #[serde(default)]
    pub uses: i64,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub expires_at: String,
}

/// `POST /api/account/friend-invites/{token}/accept`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct FriendAccepted {
    #[serde(default)]
    pub accepted: bool,
    /// The inviter's nick, when they have one.
    #[serde(default)]
    pub friend: Option<String>,
}

/// Actions `POST /api/account/friends` takes.
pub const FRIEND_ACTIONS: [&str; 8] =
    ["request", "approve", "decline", "remove", "block", "unblock", "subscribe", "unsubscribe"];

/// Pull the invite token out of whatever the person pasted: a full link
/// (`https://host/?friend=<token>`), a path (`/?friend=<token>`) or the bare
/// token. `None` for an empty paste.
pub fn invite_token_from_paste(pasted: &str) -> Option<String> {
    let text = pasted.trim();
    if text.is_empty() {
        return None;
    }
    let token = match text.find("friend=") {
        Some(at) => {
            let rest = &text[at + "friend=".len()..];
            rest.split(['&', '#', ' ']).next().unwrap_or("")
        }
        None => text,
    };
    let token = token.trim();
    (!token.is_empty() && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .then(|| token.to_string())
}

/// What a refused friend / invite call means. Wording is the UI's (translated).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FriendError {
    /// `404` — no such nick. Unknown, hidden and (for requests) blocked-by-them
    /// all look the same here.
    NoSuchUser,
    /// `400 that is you`.
    IsYou,
    /// `400 no request`.
    NoRequest,
    /// `403 blocked` — only when the caller blocked them.
    Blocked,
    /// `429`.
    RateLimited,
    /// `400 invite is not valid`.
    InviteInvalid,
    /// `401`.
    SignedOut,
    /// No answer.
    Network,
    Other,
}

#[derive(Deserialize, Default)]
struct ErrorBody {
    #[serde(default)]
    error: String,
}

/// Classify a failed friend / invite call.
pub fn friend_error(e: &CloudError) -> FriendError {
    match e {
        CloudError::Unauthorized => FriendError::SignedOut,
        CloudError::Forbidden(_) => FriendError::Blocked,
        CloudError::Transport(_) => FriendError::Network,
        CloudError::Http { status: 404, .. } => FriendError::NoSuchUser,
        CloudError::Http { status: 429, .. } => FriendError::RateLimited,
        CloudError::Http { status: 400, body } => {
            let said = serde_json::from_str::<ErrorBody>(body).unwrap_or_default().error.to_lowercase();
            if said.contains("that is you") {
                FriendError::IsYou
            } else if said.contains("no request") {
                FriendError::NoRequest
            } else if said.contains("invite") {
                FriendError::InviteInvalid
            } else {
                FriendError::Other
            }
        }
        _ => FriendError::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_roster_decodes_and_an_old_answer_reads_as_hidden() {
        let roster: FriendRoster = serde_json::from_str(
            r#"{"discoverable":true,"friends":["a"],"incoming":["b"],"outgoing":["c"],
                "subscriptions":["d"],"subscribers":["e","f"],"blocked":["g"]}"#,
        )
        .expect("roster");
        assert!(roster.discoverable);
        assert_eq!((roster.friends.len(), roster.subscribers.len()), (1, 2));
        let bare: FriendRoster = serde_json::from_str("{}").expect("bare");
        assert!(!bare.discoverable, "no answer means hidden, never public");
        assert!(bare.subscribers.is_empty());
    }

    #[test]
    fn invite_shapes_decode() {
        let made: FriendInviteCreated = serde_json::from_str(
            r#"{"token":"tok_1","url_path":"/?friend=tok_1","max_uses":1,"expires_at":"2026-10-15T00:00:00+00:00"}"#,
        )
        .expect("created");
        assert_eq!((made.token.as_str(), made.url_path.as_str()), ("tok_1", "/?friend=tok_1"));
        let rows: Vec<FriendInvite> = serde_json::from_str(
            r#"[{"token":"tok_1","max_uses":3,"uses":1,"created_at":"x","expires_at":"2026-10-15T00:00:00+00:00"}]"#,
        )
        .expect("list");
        assert_eq!((rows[0].uses, rows[0].max_uses), (1, 3));
        let accepted: FriendAccepted = serde_json::from_str(r#"{"accepted":true,"friend":null}"#).expect("accepted");
        assert!(accepted.accepted && accepted.friend.is_none());
    }

    #[test]
    fn a_pasted_link_path_or_code_yields_the_token() {
        for pasted in [
            "https://chart.example/?friend=abc123",
            "/?friend=abc123",
            "  abc123  ",
            "https://x.test/?a=1&friend=abc123&b=2",
            "https://x.test/?friend=abc123#top",
        ] {
            assert_eq!(invite_token_from_paste(pasted).as_deref(), Some("abc123"), "{pasted:?}");
        }
        for bad in ["", "   ", "https://x.test/?friend=", "not a token!", "a/b"] {
            assert_eq!(invite_token_from_paste(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn unknown_and_hidden_nicks_are_the_same_no_such_user() {
        // The server answers both with the same 404; the client has one word for it.
        let hidden = CloudError::Http { status: 404, body: r#"{"error":"no such nick"}"#.into() };
        let unknown = CloudError::Http { status: 404, body: String::new() };
        assert_eq!(friend_error(&hidden), FriendError::NoSuchUser);
        assert_eq!(friend_error(&hidden), friend_error(&unknown));
    }

    #[test]
    fn every_documented_refusal_has_its_class() {
        let bad = |body: &str| CloudError::Http { status: 400, body: body.into() };
        assert_eq!(friend_error(&bad(r#"{"error":"that is you"}"#)), FriendError::IsYou);
        assert_eq!(friend_error(&bad(r#"{"error":"no request"}"#)), FriendError::NoRequest);
        assert_eq!(friend_error(&bad(r#"{"error":"invite is not valid"}"#)), FriendError::InviteInvalid);
        assert_eq!(friend_error(&bad(r#"{"error":"surprise"}"#)), FriendError::Other);
        assert_eq!(friend_error(&CloudError::Forbidden(r#"{"error":"blocked"}"#.into())), FriendError::Blocked);
        let limited = CloudError::Http { status: 429, body: String::new() };
        assert_eq!(friend_error(&limited), FriendError::RateLimited);
        assert_eq!(friend_error(&CloudError::Unauthorized), FriendError::SignedOut);
        assert_eq!(friend_error(&CloudError::Transport("offline".into())), FriendError::Network);
    }

    #[test]
    fn every_action_the_server_takes_is_listed() {
        for action in ["request", "approve", "decline", "remove", "block", "unblock", "subscribe", "unsubscribe"] {
            assert!(FRIEND_ACTIONS.contains(&action), "{action}");
        }
    }
}
