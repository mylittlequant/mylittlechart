//! Asks the server whether a stored token is one it still accepts.
//!
//! The token file is not evidence. A token saved months ago reads exactly like
//! one saved this morning, and the app used to seed its whole signed-in state
//! from the file alone — so it showed an account name, offered account actions,
//! and 401ed on every call behind them (owner, 2026-09-04: a token from
//! 2026-03-17 that could not redeem a code and could not be signed out of).
//!
//! `GET /api/auth/me` is the cheapest question that has the real answer.

use crate::UPDATE_SERVER;

/// Outcome of asking the server about a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The server answered for this token — it is live.
    Valid,
    /// The server refused it. The session is over, whatever the file says.
    Rejected,
    /// We could not ask. Says nothing about the token, so nothing may be
    /// concluded from it — an offline start must not sign anybody out.
    Unknown,
}

/// Ask `GET /api/auth/me` whether `auth_header` still identifies someone.
pub async fn probe(client: &reqwest::Client, auth_header: &str) -> Verdict {
    let url = format!("{}/api/auth/me", UPDATE_SERVER);
    match client.get(&url).header("Authorization", auth_header).send().await {
        Ok(resp) if resp.status().is_success() => Verdict::Valid,
        Ok(resp) if resp.status().as_u16() == 401 => Verdict::Rejected,
        Ok(_) => Verdict::Unknown,
        Err(_) => Verdict::Unknown,
    }
}
