//! Fetches the resolved feature-gate set from `GET /api/entitlements`.
//!
//! Mirrors `check.rs`'s `fetch_latest` in shape: a thin request/parse
//! function, with the schedule-and-publish decision left to the caller in
//! `lib.rs` (there: `do_check`; here: `do_entitlements_check`).

use crate::state::Entitlements;
use crate::UPDATE_SERVER;

/// Fetch the feature-gate set for the caller identified by `auth_header`.
///
/// `auth_header` is `None` for a caller with no stored token — the
/// endpoint answers anonymous callers deliberately (the free set), so a
/// missing token is a normal case here, not an error.
pub async fn fetch(client: &reqwest::Client, auth_header: Option<&str>) -> Result<Entitlements, String> {
    let url = format!("{}/api/entitlements", UPDATE_SERVER);

    let mut req = client.get(&url);
    if let Some(auth) = auth_header {
        req = req.header("Authorization", auth);
    }

    let resp = req.send().await.map_err(|e| format!("Request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("Server returned {}", resp.status()));
    }

    resp.json::<Entitlements>().await.map_err(|e| format!("Parse error: {}", e))
}
